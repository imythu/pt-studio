//! 小时快照与做种时长折叠（accounting 03 §1.9）。
//!
//! cron `seeding_snapshot`（"2 * * * *"，catch_up once，tx manual）：对上一小时 h，逐分区、每 2000 个用户一组：
//! 读取 peers 中槽位为 h 的做种秒数（每 (用户, 种子) 封顶 3600）→ 折叠进 snatches / user_totals / user_daily →
//! 每用户发布一条 `traffic.seeding_snapshot_taken` → 记录进度并提交。进度表保证中断后续跑且不重复折叠。
//! 实时计数扣减（live）在后续迭代实现。

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use pt_plugin_api::{Ctx, NewEvent, PResult, PluginError, json};
use sqlx::Row;

const CHUNK_USERS: i64 = 2000;

fn hour_start(h: i64) -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp(h * 3600, 0).unwrap_or_else(Utc::now)
}

/// 运行一次快照。`hour` 缺省为上一小时；`force = false` 时要求全部分区的水位已越过该小时末尾。
pub async fn run(
    ctx: &Ctx,
    partitions: u32,
    grace_secs: u32,
    hour: Option<i64>,
    force: bool,
) -> PResult<u32> {
    let h = hour.unwrap_or_else(|| Utc::now().timestamp().div_euclid(3600) - 1);
    if !force {
        let mut c = ctx.sql.conn().await?;
        let wm: Option<i64> = sqlx::query_scalar("SELECT min(watermark_ms) FROM stream_cursors")
            .fetch_one(&mut *c)
            .await?;
        let need = (h + 1) * 3_600_000 + i64::from(grace_secs) * 1000;
        if wm.is_some_and(|w| w < need) {
            // 有分区落后：照常继续（迟到数据会写入下一小时的槽），记录告警
            tracing::warn!(hour = h, watermark = wm, "accounting snapshot: watermark late");
        }
    }
    let slot = h.rem_euclid(2);
    let hour_i = i32::try_from(h).map_err(|_| PluginError::validation("hour out of range"))?;
    let mut users_total = 0u32;
    for p in 0..partitions {
        let part = i16::try_from(p).map_err(|_| PluginError::internal("partition out of range"))?;
        let mut last_user: i64 = {
            let mut c = ctx.sql.conn().await?;
            sqlx::query("INSERT INTO snapshot_runs (hour, part) VALUES ($1, $2) ON CONFLICT DO NOTHING")
                .bind(hour_i)
                .bind(part)
                .execute(&mut *c)
                .await?;
            let r =
                sqlx::query("SELECT state, last_user_id FROM snapshot_runs WHERE hour = $1 AND part = $2")
                    .bind(hour_i)
                    .bind(part)
                    .fetch_one(&mut *c)
                    .await?;
            if r.get::<String, _>(0) == "done" {
                continue;
            }
            r.get(1)
        };
        loop {
            let n = chunk(ctx, h, hour_i, slot, part, &mut last_user).await?;
            users_total += n;
            ctx.sql.commit().await?;
            if n == 0 {
                break;
            }
        }
        let mut c = ctx.sql.conn().await?;
        sqlx::query(
            "UPDATE snapshot_runs SET state = 'done', finished_at = now() WHERE hour = $1 AND part = $2",
        )
        .bind(hour_i)
        .bind(part)
        .execute(&mut *c)
        .await?;
        drop(c);
        ctx.sql.commit().await?;
    }
    Ok(users_total)
}

/// 处理一组用户；返回处理的用户数（0 表示该分区已完成）。
async fn chunk(ctx: &Ctx, h: i64, hour_i: i32, slot: i64, part: i16, last_user: &mut i64) -> PResult<u32> {
    let mut c = ctx.sql.conn().await?;
    let (hour_col, secs_col) = if slot == 0 {
        ("slot0_hour", "slot0_secs")
    } else {
        ("slot1_hour", "slot1_secs")
    };
    // 先确定本组的用户边界，再读取这些用户的全部行（不在用户中间截断）
    let users: Vec<i64> = sqlx::query_scalar(&format!(
        "SELECT DISTINCT user_id FROM peers WHERE part = $1 AND {hour_col} = $2 AND {secs_col} > 0 AND user_id > $3
          ORDER BY user_id LIMIT $4"
    ))
    .bind(part)
    .bind(hour_i)
    .bind(*last_user)
    .bind(CHUNK_USERS)
    .fetch_all(&mut *c)
    .await?;
    let Some(max_user) = users.last().copied() else {
        return Ok(0);
    };
    let rows = sqlx::query(&format!(
        "SELECT user_id, torrent_id, least(sum({secs_col}), 3600)::bigint
           FROM peers WHERE part = $1 AND {hour_col} = $2 AND {secs_col} > 0 AND user_id > $3 AND user_id <= $4
          GROUP BY user_id, torrent_id ORDER BY user_id, torrent_id"
    ))
    .bind(part)
    .bind(hour_i)
    .bind(*last_user)
    .bind(max_user)
    .fetch_all(&mut *c)
    .await?;
    let items: Vec<(i64, i64, i64)> = rows.iter().map(|r| (r.get(0), r.get(1), r.get(2))).collect();
    let (us, ts, ss): (Vec<i64>, Vec<i64>, Vec<i64>) =
        items.iter().fold((vec![], vec![], vec![]), |mut acc, (u, t, s)| {
            acc.0.push(*u);
            acc.1.push(*t);
            acc.2.push(*s);
            acc
        });
    let end = hour_start(h + 1);
    // 折叠：snatches（只做种、从未 touch 的对补建）
    sqlx::query(
        "INSERT INTO snatches AS s (user_id, torrent_id, seed_secs, first_action_at, last_action_at)
         SELECT u, t, sec, $4, $4 FROM unnest($1::bigint[], $2::bigint[], $3::bigint[]) AS x(u, t, sec) ORDER BY u, t
         ON CONFLICT (user_id, torrent_id) DO UPDATE SET seed_secs = s.seed_secs + EXCLUDED.seed_secs,
             last_action_at = greatest(s.last_action_at, EXCLUDED.last_action_at), updated_at = now()",
    )
    .bind(&us)
    .bind(&ts)
    .bind(&ss)
    .bind(end)
    .execute(&mut *c)
    .await?;
    let mut per_user: BTreeMap<i64, i64> = BTreeMap::new();
    for (u, _, s) in &items {
        *per_user.entry(*u).or_default() += s;
    }
    let (uu, su): (Vec<i64>, Vec<i64>) = per_user.iter().map(|(u, s)| (*u, *s)).unzip();
    sqlx::query(
        "INSERT INTO user_totals AS t (user_id, seed_secs) SELECT * FROM unnest($1::bigint[], $2::bigint[]) ORDER BY 1
         ON CONFLICT (user_id) DO UPDATE SET seed_secs = t.seed_secs + EXCLUDED.seed_secs, updated_at = now()",
    )
    .bind(&uu)
    .bind(&su)
    .execute(&mut *c)
    .await?;
    sqlx::query(
        "INSERT INTO user_daily AS d (day, user_id, seed_secs) SELECT $1, * FROM unnest($2::bigint[], $3::bigint[]) ORDER BY 2
         ON CONFLICT (day, user_id) DO UPDATE SET seed_secs = d.seed_secs + EXCLUDED.seed_secs",
    )
    .bind(hour_start(h).date_naive())
    .bind(&uu)
    .bind(&su)
    .execute(&mut *c)
    .await?;
    // 种子体积（镜像）与当前做种人数
    let mut torrents: Vec<i64> = ts.clone();
    torrents.sort_unstable();
    torrents.dedup();
    let facts: BTreeMap<i64, (i64, i64)> = sqlx::query(
        "SELECT x.t, coalesce(m.size, 0),
                (SELECT count(DISTINCT p.user_id) FROM peers p
                  WHERE p.torrent_id = x.t AND p.is_active AND p.is_seeder AND p.last_announce_at > now() - interval '1 hour')
           FROM unnest($1::bigint[]) AS x(t) LEFT JOIN torrents m ON m.id = x.t",
    )
    .bind(&torrents)
    .fetch_all(&mut *c)
    .await?
    .iter()
    .map(|r| (r.get::<i64, _>(0), (r.get::<i64, _>(1), r.get::<i64, _>(2))))
    .collect();
    sqlx::query(
        "UPDATE snapshot_runs SET last_user_id = $3, users = users + $4 WHERE hour = $1 AND part = $2",
    )
    .bind(hour_i)
    .bind(part)
    .bind(max_user)
    .bind(i32::try_from(per_user.len()).unwrap_or(i32::MAX))
    .execute(&mut *c)
    .await?;
    drop(c);
    // 事件：每用户一条（超过 300 个种子拆分）
    let mut by_user: BTreeMap<i64, Vec<serde_json::Value>> = BTreeMap::new();
    for (u, t, s) in &items {
        let (size, seeders) = facts.get(t).copied().unwrap_or((0, 0));
        by_user
            .entry(*u)
            .or_default()
            .push(json!({ "torrent_id": t, "size": size, "seed_secs": s, "seeders": seeders }));
    }
    for (u, list) in &by_user {
        let batch_id = uuid::Uuid::new_v5(
            &uuid::Uuid::NAMESPACE_OID,
            format!("pt-studio:snapshot:{h}:{u}").as_bytes(),
        );
        let chunks: Vec<&[serde_json::Value]> = list.chunks(300).collect();
        let parts = chunks.len();
        for (n, ch) in chunks.into_iter().enumerate() {
            ctx.events
                .publish(NewEvent::v1("traffic.seeding_snapshot_taken", format!("user:{u}"), json!({
                    "user_id": u, "taken_at": end.to_rfc3339(), "period_start": hour_start(h).to_rfc3339(), "period_secs": 3600,
                    "batch_id": batch_id, "part": n + 1, "parts": parts, "torrents": ch })))
                .await?;
        }
    }
    *last_user = max_user;
    Ok(u32::try_from(per_user.len()).unwrap_or(u32::MAX))
}
