//! top10 插件（WASM，docs/plugins/top10/README.md）：排行榜。
//!
//! - 种子榜：完成数最多（日 / 周 / 月 / 全部）、当前最活跃（做种 + 下载）、当前做种最多。
//! - 用户榜（可在设置中关闭）：上传量、做种时长（周 / 月 / 全部）、发布数（周 / 月 / 全部，不计匿名发布）。
//!   用户可选择退出用户榜；退出立即生效（读取时也过滤）。
//! - 每小时 `refresh` 整体重算前 N 名写入 `entries`；读取只查快照。

pt_plugin_sdk::world!("top10", http);

use exports::pt::plugin::{http_handler, on_cron};
use pt_plugin_sdk::{prelude::*, sql};

struct Top10;

pub struct Board {
    pub key: &'static str,
    pub kind: &'static str,
    pub periods: &'static [&'static str],
}

pub const BOARDS: &[Board] = &[
    Board { key: "torrents.snatched", kind: "torrent", periods: &["day", "week", "month", "all"] },
    Board { key: "torrents.active", kind: "torrent", periods: &["now"] },
    Board { key: "torrents.seeded", kind: "torrent", periods: &["now"] },
    Board { key: "users.uploaded", kind: "user", periods: &["week", "month", "all"] },
    Board { key: "users.seed_time", kind: "user", periods: &["week", "month", "all"] },
    Board { key: "users.uploads", kind: "user", periods: &["week", "month", "all"] },
];

pub fn days(period: &str) -> i64 {
    match period {
        "day" => 1,
        "week" => 7,
        "month" => 30,
        _ => 0,
    }
}

struct Settings {
    size: i64,
    user_boards: bool,
}

fn settings() -> Settings {
    let s = ctx::settings();
    Settings { size: s["size"].as_i64().unwrap_or(10).clamp(1, 100), user_boards: s["user_boards"].as_bool().unwrap_or(true) }
}

/// 某个榜单 / 周期的排名查询：返回 (entity_id, value)，已按 value 降序
fn source_sql(board: &str, period: &str) -> Option<String> {
    let d = days(period);
    // 只统计已发布的种子；用户榜排除已退出与非正常状态的用户
    const LIVE_USER: &str = "JOIN identity.v_user_public u ON u.id = x.user_id AND u.status = 'active'
                             WHERE NOT EXISTS (SELECT 1 FROM opt_out o WHERE o.user_id = x.user_id)";
    Some(match (board, period) {
        ("torrents.snatched", "all") => "SELECT p.torrent_id, p.completed FROM accounting.v_torrent_peers p
              JOIN torrent.v_torrent_public t ON t.id = p.torrent_id AND t.status = 'approved'
             WHERE p.completed > 0 ORDER BY p.completed DESC, p.torrent_id DESC LIMIT $1"
            .into(),
        ("torrents.snatched", _) => format!(
            "SELECT s.torrent_id, count(*) FROM accounting.v_snatch s
               JOIN torrent.v_torrent_public t ON t.id = s.torrent_id AND t.status = 'approved'
              WHERE s.completed_at > now() - interval '{d} days'
              GROUP BY s.torrent_id ORDER BY count(*) DESC, s.torrent_id DESC LIMIT $1"
        ),
        ("torrents.active", _) => "SELECT p.torrent_id, p.seeders + p.leechers FROM accounting.v_torrent_peers p
              JOIN torrent.v_torrent_public t ON t.id = p.torrent_id AND t.status = 'approved'
             WHERE p.seeders + p.leechers > 0 ORDER BY p.seeders + p.leechers DESC, p.torrent_id DESC LIMIT $1"
            .into(),
        ("torrents.seeded", _) => "SELECT p.torrent_id, p.seeders FROM accounting.v_torrent_peers p
              JOIN torrent.v_torrent_public t ON t.id = p.torrent_id AND t.status = 'approved'
             WHERE p.seeders > 0 ORDER BY p.seeders DESC, p.torrent_id DESC LIMIT $1"
            .into(),
        ("users.uploaded" | "users.seed_time", "all") => {
            let col = if board == "users.uploaded" { "uploaded" } else { "seed_secs" };
            format!(
                "SELECT x.user_id, x.{col} FROM accounting.v_user_totals x {LIVE_USER} AND x.{col} > 0
                  ORDER BY x.{col} DESC, x.user_id LIMIT $1"
            )
        }
        ("users.uploaded" | "users.seed_time", _) => {
            let col = if board == "users.uploaded" { "uploaded" } else { "seed_secs" };
            format!(
                "SELECT x.user_id, x.v FROM (SELECT user_id, sum({col})::bigint AS v FROM accounting.v_user_daily
                   WHERE day > current_date - {d} GROUP BY user_id) x {LIVE_USER} AND x.v > 0
                  ORDER BY x.v DESC, x.user_id LIMIT $1"
            )
        }
        ("users.uploads", _) => {
            let since = if d > 0 { format!("AND created_at > now() - interval '{d} days'") } else { String::new() };
            format!(
                "SELECT x.user_id, x.n FROM (SELECT uploader_id AS user_id, count(*) AS n FROM torrent.v_torrent_public
                   WHERE status = 'approved' AND uploader_id IS NOT NULL {since} GROUP BY uploader_id) x {LIVE_USER}
                  ORDER BY x.n DESC, x.user_id LIMIT $1"
            )
        }
        _ => return None,
    })
}

fn refresh() -> R<()> {
    let s = settings();
    sql::execute("DELETE FROM entries", &[])?;
    for b in BOARDS {
        if b.kind == "user" && !s.user_boards {
            continue;
        }
        for p in b.periods {
            let Some(q) = source_sql(b.key, p) else { continue };
            let rows = sql::query(&q, &[SqlValue::Int(s.size)])?;
            for (i, row) in rows.rows.iter().enumerate() {
                #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
                let rank = i as i64 + 1;
                sql::execute(
                    "INSERT INTO entries (board, period, rank, entity_id, value) VALUES ($1, $2, $3, $4, $5)",
                    &[
                        SqlValue::Text(b.key.into()),
                        SqlValue::Text((*p).into()),
                        SqlValue::Int(rank),
                        SqlValue::Int(row.int(0).unwrap_or(0)),
                        SqlValue::Int(row.int(1).unwrap_or(0)),
                    ],
                )?;
            }
        }
    }
    sql::execute(
        "INSERT INTO refreshes (id, refreshed_at) VALUES (true, now()) ON CONFLICT (id) DO UPDATE SET refreshed_at = now()",
        &[],
    )?;
    sql::commit()?;
    Ok(())
}

fn boards() -> R<Value> {
    let s = settings();
    let rows = sql::query(
        "SELECT e.board, e.period, e.rank, e.entity_id, e.value, t.title, u.username
           FROM entries e
           LEFT JOIN torrent.v_torrent_public t ON e.board LIKE 'torrents.%' AND t.id = e.entity_id
           LEFT JOIN identity.v_user_public u ON e.board LIKE 'users.%' AND u.id = e.entity_id
          WHERE (e.board LIKE 'torrents.%' AND t.status = 'approved')
             OR (e.board LIKE 'users.%' AND $1 AND NOT EXISTS (SELECT 1 FROM opt_out o WHERE o.user_id = e.entity_id))
          ORDER BY e.board, e.period, e.rank",
        &[SqlValue::Boolean(s.user_boards)],
    )?;
    let out: Vec<Value> = BOARDS
        .iter()
        .filter(|b| b.kind == "torrent" || s.user_boards)
        .map(|b| {
            let periods: Vec<Value> = b
                .periods
                .iter()
                .map(|p| {
                    let items: Vec<Value> = rows
                        .rows
                        .iter()
                        .filter(|r| r.text(0).as_deref() == Some(b.key) && r.text(1).as_deref() == Some(p))
                        .enumerate()
                        .map(|(i, r)| {
                            json!({ "rank": i + 1, "id": r.int(3), "value": r.int(4),
                                    "name": if b.kind == "torrent" { r.text(5) } else { r.text(6) } })
                        })
                        .collect();
                    json!({ "period": p, "items": items })
                })
                .collect();
            json!({ "key": b.key, "kind": b.kind, "periods": periods })
        })
        .collect();
    let at = sql::query("SELECT (extract(epoch FROM refreshed_at) * 1000)::bigint FROM refreshes", &[])?;
    Ok(json!({ "boards": out, "refreshed_at": at.rows.first().and_then(|r| r.int(0)), "user_boards": s.user_boards }))
}

impl on_cron::Guest for Top10 {
    fn run(job: on_cron::JobInvocation) -> R<()> {
        match job.name.as_str() {
            "refresh" => refresh(),
            _ => Err(err("kernel.not_found", "unknown job")),
        }
    }
}

impl http_handler::Guest for Top10 {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let reply = match req.route() {
            "boards" => http::Reply::ok(&boards()?),
            "me" => {
                let uid = ctx::current_user()?;
                let n = sql::query("SELECT 1 FROM opt_out WHERE user_id = $1", &[SqlValue::Int(uid)])?;
                http::Reply::ok(&json!({ "opted_out": !n.rows.is_empty() }))
            }
            "opt_out" => {
                let uid = ctx::current_user()?;
                let out = req.body_value()["opted_out"].as_bool().unwrap_or(true);
                if out {
                    sql::execute("INSERT INTO opt_out (user_id) VALUES ($1) ON CONFLICT DO NOTHING", &[SqlValue::Int(uid)])?;
                } else {
                    sql::execute("DELETE FROM opt_out WHERE user_id = $1", &[SqlValue::Int(uid)])?;
                }
                http::Reply::ok(&json!({ "opted_out": out }))
            }
            _ => return Err(err("kernel.not_found", "no such route")),
        };
        Ok(reply.into())
    }
}

export!(Top10);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_board_period_has_a_query() {
        for b in BOARDS {
            for p in b.periods {
                let q = source_sql(b.key, p).unwrap_or_default();
                assert!(q.contains("LIMIT $1"), "{} {p}", b.key);
            }
        }
        assert_eq!(days("week"), 7);
        assert!(source_sql("nope", "all").is_none());
    }
}
