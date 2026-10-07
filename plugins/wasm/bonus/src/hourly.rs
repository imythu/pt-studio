//! 小时结算与批量入账（bonus 03 §1.3、§1.4、§1.6）。

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::{
    R, err,
    expr::Env,
    int, ledger,
    pt::plugin::{
        events, log, services,
        sql::{self, SqlValue},
    },
    rules::{self, CompiledRules, RuleDef, TORRENT_VARS},
    text,
};

fn num(v: &SqlValue) -> f64 {
    match v {
        SqlValue::Int(i) => *i as f64,
        SqlValue::Float(f) => *f,
        SqlValue::Numeric(s) => s.parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// 当前生效的规则版本与定义
pub fn version_at(period_start: &str) -> R<Option<(i64, RuleDef)>> {
    let r = sql::query(
        "SELECT version, definition FROM rule_versions WHERE effective_from <= $1::timestamptz AND NOT is_revoked
          ORDER BY effective_from DESC LIMIT 1",
        &[SqlValue::Text(period_start.into())],
    )?;
    let Some(row) = r.rows.first() else {
        return Ok(None);
    };
    let SqlValue::Json(def) = &row[1] else {
        return Err(err("bonus.rules_invalid", "rule definition is not JSON"));
    };
    let def: RuleDef = serde_json::from_str(def)
        .map_err(|e| err("bonus.rules_invalid", &format!("stored rule definition: {e}")))?;
    Ok(Some((int(&row[0]), def)))
}

pub fn currencies() -> R<BTreeMap<String, u32>> {
    let r = sql::query("SELECT code, scale FROM currencies WHERE is_active", &[])?;
    Ok(r.rows
        .iter()
        .map(|row| (text(&row[0]), u32::try_from(int(&row[1])).unwrap_or(0)))
        .collect())
}

pub fn compiled(def: &RuleDef) -> R<CompiledRules> {
    let curs: Vec<String> = currencies()?.into_keys().collect();
    rules::compile(def, &curs).map_err(|e| {
        let msg = e
            .iter()
            .map(|(at, i)| format!("{at}: {} {}", i.code, i.message))
            .collect::<Vec<_>>()
            .join("; ");
        err("bonus.rules_invalid", &msg)
    })
}

/// 种子事实（镜像）
#[derive(Debug, Clone, Default)]
struct Fact {
    size: f64,
    type_id: String,
    uploader_id: i64,
    created_at_secs: Option<f64>,
    deleted: bool,
    leechers: f64,
    completed: f64,
    tags: Vec<String>,
}

struct TorrentEnv<'a> {
    vals: Vec<f64>,
    fact: &'a Fact,
}

impl Env for TorrentEnv<'_> {
    fn var(&self, i: usize) -> f64 {
        self.vals.get(i).copied().unwrap_or(0.0)
    }
    fn has_tag(&self, t: &str) -> bool {
        self.fact.tags.iter().any(|x| x == t)
    }
    fn is_type(&self, t: &str) -> bool {
        self.fact.type_id == t
    }
}

struct UserEnv {
    vals: Vec<f64>,
    roles: Vec<String>,
}

impl Env for UserEnv {
    fn var(&self, i: usize) -> f64 {
        self.vals.get(i).copied().unwrap_or(0.0)
    }
    fn has_role(&self, r: &str) -> bool {
        self.roles.iter().any(|x| x == r)
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Partial {
    pub sums: BTreeMap<String, f64>,
    pub count: i64,
    pub total_size: f64,
    pub hours: f64,
    pub flags: BTreeMap<String, bool>,
}

fn load_facts(ids: &[i64]) -> R<BTreeMap<i64, Fact>> {
    let r = sql::query(
        "SELECT f.torrent_id, f.size, coalesce(f.type_id, ''), coalesce(f.uploader_id, 0),
                extract(epoch FROM f.source_created_at)::float8, f.is_deleted, f.leechers, f.completed,
                coalesce((SELECT array_agg(tag) FROM torrent_tags t WHERE t.torrent_id = f.torrent_id), '{}')
           FROM torrent_facts f WHERE f.torrent_id = ANY($1)",
        &[SqlValue::IntArray(ids.to_vec())],
    )?;
    Ok(r.rows
        .iter()
        .map(|row| {
            let created = match &row[4] {
                SqlValue::Float(f) => Some(*f),
                _ => None,
            };
            let tags = match &row[8] {
                SqlValue::TextArray(a) => a.clone(),
                _ => vec![],
            };
            (
                int(&row[0]),
                Fact {
                    size: num(&row[1]),
                    type_id: text(&row[2]),
                    uploader_id: int(&row[3]),
                    created_at_secs: created,
                    deleted: matches!(row[5], SqlValue::Boolean(true)),
                    leechers: num(&row[6]),
                    completed: num(&row[7]),
                    tags,
                },
            )
        })
        .collect())
}

/// 部分聚合（03 §1.4 partial_aggregate）
fn partial(c: &CompiledRules, user: i64, period_secs: f64, torrents: &[Value]) -> R<Partial> {
    let rows: Vec<&Value> = torrents
        .iter()
        .filter(|t| t["seed_secs"].as_f64().unwrap_or(0.0) >= c.filters.min_seed_secs)
        .collect();
    let ids: Vec<i64> = rows.iter().filter_map(|t| t["torrent_id"].as_i64()).collect();
    let facts = load_facts(&ids)?;
    let snatch: BTreeMap<i64, f64> = if c.needs_snatch {
        sql::query(
            "SELECT torrent_id, seed_secs FROM accounting.v_snatch WHERE user_id = $1 AND torrent_id = ANY($2)",
            &[SqlValue::Int(user), SqlValue::IntArray(ids.clone())],
        )?
        .rows
        .iter()
        .map(|r| (int(&r[0]), num(&r[1])))
        .collect()
    } else {
        BTreeMap::new()
    };
    let mut p = Partial::default();
    for t in &c.terms {
        p.sums.insert(t.name.clone(), 0.0);
    }
    for t in rows {
        let tid = t["torrent_id"].as_i64().unwrap_or(0);
        let missing = Fact::default();
        let f = facts.get(&tid).unwrap_or_else(|| {
            p.flags.insert("facts_missing".into(), true);
            &missing
        });
        let snap_size = t["size"].as_f64().unwrap_or(0.0);
        let size = if snap_size > 0.0 { snap_size } else { f.size };
        if size < c.filters.min_size || f.deleted {
            continue;
        }
        let seed_secs = t["seed_secs"].as_f64().unwrap_or(0.0).clamp(0.0, 3600.0);
        let age = f.created_at_secs.map_or(0.0, |cr| (period_secs - cr).max(0.0));
        let mut vals = vec![0.0; c.torrent_vars];
        let base = [
            size,
            size / 1_073_741_824.0,
            seed_secs,
            seed_secs / 3600.0,
            t["seeders"].as_f64().unwrap_or(0.0),
            age,
            age / 86_400.0,
            age / 604_800.0,
            if f.uploader_id == user { 1.0 } else { 0.0 },
            f.leechers,
            f.completed,
            snatch.get(&tid).copied().unwrap_or(0.0),
        ];
        vals[..TORRENT_VARS.len()].copy_from_slice(&base);
        let mut env = TorrentEnv { vals, fact: f };
        for term in &c.terms {
            let mut nf = false;
            let v = term.expr.eval(&env, &mut nf).unwrap_or_else(|_| {
                p.flags.insert("eval_errors".into(), true);
                0.0
            });
            if nf {
                p.flags.insert("nonfinite".into(), true);
            }
            env.vals[term.slot] = v;
            *p.sums.entry(term.name.clone()).or_default() += v;
        }
        p.count += 1;
        p.total_size += size;
        p.hours += seed_secs / 3600.0;
    }
    Ok(p)
}

fn merge(parts: Vec<Partial>) -> Partial {
    let mut out = Partial::default();
    for p in parts {
        for (k, v) in p.sums {
            *out.sums.entry(k).or_default() += v;
        }
        out.count += p.count;
        out.total_size += p.total_size;
        out.hours += p.hours;
        out.flags.extend(p.flags);
    }
    out
}

/// `traffic.seeding_snapshot_taken` → 用户小时（03 §1.4）
pub fn on_snapshot(event_id: &str, payload: &Value) -> R<()> {
    let user = payload["user_id"]
        .as_i64()
        .ok_or_else(|| err("bonus.snapshot_invalid", "user_id missing"))?;
    let period = payload["period_start"]
        .as_str()
        .ok_or_else(|| err("bonus.snapshot_invalid", "period_start missing"))?
        .to_owned();
    let torrents = payload["torrents"].as_array().cloned().unwrap_or_default();
    let parts = payload["parts"].as_i64().unwrap_or(1);
    let period_secs: f64 = num(&sql::query(
        "SELECT extract(epoch FROM $1::timestamptz)::float8",
        &[SqlValue::Text(period.clone())],
    )?
    .rows[0][0]);
    let _ = event_id;
    let exists = !sql::query(
        "SELECT 1 FROM user_hours WHERE period_start = $1::timestamptz AND user_id = $2",
        &[SqlValue::Text(period.clone()), SqlValue::Int(user)],
    )?
    .rows
    .is_empty();
    if exists {
        log::emit(log::Level::Warn, "duplicate user hour ignored", &[]);
        return Ok(());
    }
    let Some((ver, def)) = version_at(&period)? else {
        return Err(err(
            "bonus.rules_missing",
            "no rule version is effective for this hour",
        ));
    };
    let c = compiled(&def)?;
    let mine = partial(&c, user, period_secs, &torrents)?;
    let agg = if parts > 1 {
        let batch = payload["batch_id"].as_str().unwrap_or_default().to_owned();
        sql::execute(
            "INSERT INTO snapshot_parts (user_id, period_start, batch_id, part, parts, rule_version, partial)
             VALUES ($1, $2::timestamptz, $3::uuid, $4, $5, $6, $7) ON CONFLICT DO NOTHING",
            &[
                SqlValue::Int(user),
                SqlValue::Text(period.clone()),
                SqlValue::Text(batch.clone()),
                SqlValue::Int(payload["part"].as_i64().unwrap_or(1)),
                SqlValue::Int(parts),
                SqlValue::Int(ver),
                SqlValue::Json(serde_json::to_string(&mine).unwrap_or_default()),
            ],
        )?;
        let got = sql::query(
            "SELECT partial FROM snapshot_parts WHERE user_id = $1 AND period_start = $2::timestamptz AND batch_id = $3::uuid",
            &[
                SqlValue::Int(user),
                SqlValue::Text(period.clone()),
                SqlValue::Text(batch.clone()),
            ],
        )?;
        if i64::try_from(got.rows.len()).unwrap_or(0) < parts {
            return Ok(());
        }
        let all: Vec<Partial> = got
            .rows
            .iter()
            .filter_map(|r| match &r[0] {
                SqlValue::Json(j) => serde_json::from_str(j).ok(),
                _ => None,
            })
            .collect();
        sql::execute(
            "DELETE FROM snapshot_parts WHERE user_id = $1 AND period_start = $2::timestamptz AND batch_id = $3::uuid",
            &[
                SqlValue::Int(user),
                SqlValue::Text(period.clone()),
                SqlValue::Text(batch),
            ],
        )?;
        merge(all)
    } else {
        mine
    };
    finalize(user, &period, ver, &c, agg)
}

fn finalize(user: i64, period: &str, ver: i64, c: &CompiledRules, agg: Partial) -> R<()> {
    let u: Value = serde_json::from_str(&services::call(
        "ctx.users",
        "get",
        &json!({ "id": user }).to_string(),
    )?)
    .unwrap_or(Value::Null);
    let reason = match u["status"].as_str() {
        None => Some("user_missing"),
        Some("banned") => Some("user_banned"),
        Some("deleted") => Some("user_deleted"),
        Some("pending") => Some("user_pending"),
        Some("leech_disabled") => Some("leech_disabled"),
        Some(_) => None,
    };
    let curs = currencies()?;
    let mut amounts: BTreeMap<String, i64> = BTreeMap::new();
    let mut flags = agg.flags.clone();
    if reason.is_none() {
        let mut vals = vec![
            agg.count as f64,
            agg.total_size / 1_073_741_824.0,
            agg.hours,
            u["class_id"].as_f64().unwrap_or(0.0),
            0.0, // medal_factor：ctx.medals 接入后提供
            0.0, // invitee_seed_points：P2
        ];
        for t in &c.terms {
            vals.push(agg.sums.get(&t.name).copied().unwrap_or(0.0));
        }
        if c.needs_roles {
            // ctx.rbac.roles_of 尚未提供：has_role 一律为假
            flags.insert("roles_unavailable".into(), true);
        }
        let env = UserEnv { vals, roles: vec![] };
        for (cur, f) in &c.formulas {
            let mut nf = false;
            let v = f.eval(&env, &mut nf).unwrap_or(0.0);
            if nf {
                flags.insert("nonfinite".into(), true);
            }
            let scale = 10f64.powi(i32::try_from(curs.get(cur).copied().unwrap_or(0)).unwrap_or(0));
            let mut m = if v > 0.0 {
                (v * scale).floor().min(9_007_199_254_740_991.0) as i64
            } else {
                0
            };
            if let Some(cap) = c.caps.get(cur) {
                m = m.min(*cap);
            }
            amounts.insert(cur.clone(), m);
        }
    }
    let terms_json = json!(agg.sums);
    sql::execute(
        "INSERT INTO user_hours (period_start, user_id, rule_version, torrent_count, total_size, terms, amounts, skipped_reason, flags, source)
         VALUES ($1::timestamptz, $2, $3, $4, $5, $6, $7, $8, $9, 'live') ON CONFLICT DO NOTHING",
        &[SqlValue::Text(period.into()), SqlValue::Int(user), SqlValue::Int(ver), SqlValue::Int(agg.count),
          SqlValue::Int(agg.total_size as i64), SqlValue::Json(terms_json.to_string()),
          SqlValue::Json(json!(amounts).to_string()),
          reason.map_or(SqlValue::Null, |r| SqlValue::Text(r.into())), SqlValue::Json(json!(flags).to_string())],
    )?;
    for (cur, amount) in &amounts {
        if *amount > 0 {
            sql::execute(
                "INSERT INTO accruals (user_id, currency, period_start, amount) VALUES ($1, $2, $3::timestamptz, $4) ON CONFLICT DO NOTHING",
                &[
                    SqlValue::Int(user),
                    SqlValue::Text(cur.clone()),
                    SqlValue::Text(period.into()),
                    SqlValue::Int(*amount),
                ],
            )?;
        }
    }
    if amounts.values().any(|a| *a > 0) {
        events::publish(&events::NewEvent {
            event_type: "bonus.hourly_settled".into(),
            version: 1,
            key: format!("user:{user}"),
            payload: json!({ "user_id": user, "period_start": period, "amount": amounts.get("bonus").copied().unwrap_or(0),
                             "currency": "bonus", "amounts": amounts, "rule_version": ver, "torrent_count": agg.count,
                             "terms": agg.sums, "breakdown": { "count": agg.count, "total_gib": agg.total_size / 1_073_741_824.0,
                             "seed_hours": agg.hours } })
            .to_string(),
        })?;
    }
    Ok(())
}

/// 批量入账（03 §1.6）。`cutoff` 缺省为当前整点；只入账 `period_start < cutoff` 的应计。
pub fn post_accruals(cutoff: Option<String>) -> R<()> {
    // 0 续跑未完成的入账批
    for row in sql::query(
        "SELECT id FROM postings WHERE state IN ('open', 'submitting') ORDER BY id",
        &[],
    )?
    .rows
    {
        submit(int(&row[0]))?;
    }
    let cutoff = match cutoff {
        Some(c) => c,
        None => text(&sql::query("SELECT date_trunc('hour', now())::text", &[])?.rows[0][0]),
    };
    for cur in currencies()?.into_keys() {
        let exists = sql::query(
            "SELECT 1 FROM postings WHERE currency = $1 AND cutoff = $2::timestamptz",
            &[SqlValue::Text(cur.clone()), SqlValue::Text(cutoff.clone())],
        )?;
        if !exists.rows.is_empty() {
            continue;
        }
        let pending = sql::query(
            "SELECT 1 FROM accruals WHERE currency = $1 AND posting_id IS NULL AND period_start < $2::timestamptz LIMIT 1",
            &[SqlValue::Text(cur.clone()), SqlValue::Text(cutoff.clone())],
        )?;
        if pending.rows.is_empty() {
            continue;
        }
        let pid = int(&sql::query("INSERT INTO postings (currency, cutoff, state) VALUES ($1, $2::timestamptz, 'open') RETURNING id",
                                  &[SqlValue::Text(cur.clone()), SqlValue::Text(cutoff.clone())])?.rows[0][0]);
        sql::commit()?;
        loop {
            let n = sql::execute(
                "UPDATE accruals SET posting_id = $1 WHERE (user_id, currency, period_start) IN
                   (SELECT user_id, currency, period_start FROM accruals WHERE currency = $2 AND posting_id IS NULL
                     AND period_start < $3::timestamptz ORDER BY user_id LIMIT 50000)",
                &[SqlValue::Int(pid), SqlValue::Text(cur.clone()), SqlValue::Text(cutoff.clone())],
            )?;
            sql::commit()?;
            if n == 0 {
                break;
            }
        }
        sql::execute(
            "INSERT INTO posting_items (posting_id, user_id, amount, hours)
             SELECT $1, user_id, sum(amount), count(*) FROM accruals WHERE posting_id = $1 GROUP BY user_id",
            &[SqlValue::Int(pid)],
        )?;
        sql::execute(
            "UPDATE postings SET state = 'submitting', updated_at = now(),
                    item_count = (SELECT count(*) FROM posting_items WHERE posting_id = $1),
                    total_amount = (SELECT coalesce(sum(amount), 0) FROM posting_items WHERE posting_id = $1)
              WHERE id = $1",
            &[SqlValue::Int(pid)],
        )?;
        sql::commit()?;
        submit(pid)?;
    }
    Ok(())
}

fn submit(pid: i64) -> R<()> {
    let p = sql::query(
        "SELECT currency, cutoff::text FROM postings WHERE id = $1",
        &[SqlValue::Int(pid)],
    )?;
    let (cur, cutoff) = (text(&p.rows[0][0]), text(&p.rows[0][1]));
    loop {
        let items = sql::query(
            "SELECT user_id, amount FROM posting_items WHERE posting_id = $1 AND state = 'pending' ORDER BY user_id LIMIT 500",
            &[SqlValue::Int(pid)],
        )?;
        if items.rows.is_empty() {
            break;
        }
        let reqs: Vec<Value> = items
            .rows
            .iter()
            .map(|r| {
                json!({ "currency": cur, "from_user": null, "to_user": int(&r[0]), "amount": int(&r[1]),
                             "reason": "bonus.seeding", "memo": format!("seeding until {cutoff}"),
                             "idempotency_key": format!("post:{pid}:{}", int(&r[0])) })
            })
            .collect();
        let results = ledger("transfer_batch", &json!({ "requests": reqs }))?;
        let mut progressed = false;
        for (r, res) in items
            .rows
            .iter()
            .zip(results.as_array().cloned().unwrap_or_default())
        {
            let uid = int(&r[0]);
            if let Some(ok) = res.get("ok") {
                progressed = true;
                sql::execute(
                    "UPDATE posting_items SET state = 'done', entry_id = $3::uuid, updated_at = now() WHERE posting_id = $1 AND user_id = $2",
                    &[
                        SqlValue::Int(pid),
                        SqlValue::Int(uid),
                        SqlValue::Text(ok["entry_id"].as_str().unwrap_or_default().into()),
                    ],
                )?;
            } else {
                let code = res["err"]["code"].as_str().unwrap_or("unknown").to_owned();
                sql::execute(
                    "UPDATE posting_items SET attempts = attempts + 1, error_code = $3,
                            state = CASE WHEN attempts + 1 >= 5 THEN 'failed' ELSE state END, updated_at = now()
                      WHERE posting_id = $1 AND user_id = $2",
                    &[SqlValue::Int(pid), SqlValue::Int(uid), SqlValue::Text(code)],
                )?;
            }
        }
        sql::commit()?;
        if !progressed {
            // 本批全部失败：留待下次运行重试（attempts 达到 5 次后标记 failed）
            break;
        }
    }
    sql::execute(
        "UPDATE postings SET state = CASE WHEN EXISTS (SELECT 1 FROM posting_items WHERE posting_id = $1 AND state = 'failed')
                                          THEN 'done_with_failures' ELSE 'done' END, updated_at = now()
          WHERE id = $1 AND NOT EXISTS (SELECT 1 FROM posting_items WHERE posting_id = $1 AND state = 'pending')",
        &[SqlValue::Int(pid)],
    )?;
    sql::commit()
}
