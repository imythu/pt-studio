//! 多账号关联：账号图的边有两种来源——同网段对端证据（`anticheat.shared_ip_counterparty`，同一种子 swarm 中同网段）
//! 与 identity 记录的共用登录 IP（`ctx.users.shared_ip_pairs`，站点授权 `user.ip` 后启用）；
//! 连通分量即疑似同一人的账号簇；复核员把账号对标记为“同一人”或“无关”，“无关”的对之后不再产生同网段证据。
//! “同一人”结论给两个账号各写一条证据 `anticheat.same_person_confirmed`（默认规则 `same_person` 计分），改判 / 撤销时标为误判。
//! 只用用户 ID、种子 ID 与共用 IP 的个数，本插件不接触 IP 本身。

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use pt_plugin_sdk::{
    R, err,
    serde_json::{Value, json},
};

use crate::api::usernames;
use crate::core::{self, Trigger};
use crate::db::{self, RowGet, i, ints, j, ms, q, q1, t, x};

const VERDICTS: &[&str] = &["same_person", "unrelated"];
pub const SAME_PERSON_KIND: &str = "anticheat.same_person_confirmed";
const MAX_MEMBERS: usize = 50;
const MAX_EDGES: usize = 200;
const SOURCES: &[&str] = &["all", "swarm", "login"];
/// 被超过这么多账号用过的登录 IP（公司 / 校园 NAT、代理）不连边
const LOGIN_IP_MAX_USERS: i64 = 8;

/// (user_a, user_b, 共用登录 IP 数, 最近共用时间 ms)
type LoginPair = (i64, i64, i64, i64);
type Graph = BTreeMap<(i64, i64), Edge>;

#[derive(Debug, Default, Clone)]
pub struct Edge {
    pub torrents: BTreeSet<i64>,
    pub hits: i64,
    pub last_at: i64,
    /// 共用登录 IP 个数
    pub login_ips: i64,
    pub verdict: Option<String>,
    pub note: Option<String>,
    pub reviewed_at: Option<i64>,
}

fn pair(a: i64, b: i64) -> (i64, i64) {
    if a < b { (a, b) } else { (b, a) }
}

/// 边是否连通（“无关”不连；确认同一人总连；否则同网段种子数达到门槛或共用过登录 IP）
fn linked(e: &Edge, min_torrents: usize) -> bool {
    match e.verdict.as_deref() {
        Some("unrelated") => false,
        Some("same_person") => true,
        _ => e.torrents.len() >= min_torrents.max(1) || e.login_ips > 0,
    }
}

/// `ctx.users.shared_ip_pairs`：未授权 `user.ip` 或 identity 不可用时 None
fn login_pairs(days: i64, focus: Option<i64>) -> R<Option<Vec<LoginPair>>> {
    let v = match db::call(
        "ctx.users",
        "shared_ip_pairs",
        &json!({ "days": days, "user": focus, "max_users": LOGIN_IP_MAX_USERS, "limit": 20000 }),
    ) {
        Ok(v) => v,
        Err(e)
            if e.code == "kernel.capability_denied" || db::retryable(&e) || e.code == "kernel.not_found" =>
        {
            return Ok(None);
        }
        Err(e) => return Err(e),
    };
    Ok(Some(
        v["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| {
                Some((
                    p["a"].as_i64()?,
                    p["b"].as_i64()?,
                    p["shared_ips"].as_i64()?,
                    p["last_seen_at"].as_i64().unwrap_or(0),
                ))
            })
            .collect(),
    ))
}

/// 最近 `days` 天的边（排除误判证据）+ 复核过的账号对；`focus` 时只取与该用户相连的边。
/// `source`：all / swarm（只用同网段证据）/ login（只用共用登录 IP）。返回的布尔值表示登录 IP 来源是否可用
fn edges(days: i64, focus: Option<i64>, source: &str) -> R<(Graph, bool)> {
    if !(1..=365).contains(&days) {
        return Err(db::validation("days must be 1..=365"));
    }
    if !SOURCES.contains(&source) {
        return Err(db::validation("source must be all, swarm or login"));
    }
    let mut out = Graph::new();
    if source != "login" {
        swarm_edges(days, focus, &mut out)?;
    }
    let login = if source == "swarm" {
        None
    } else {
        login_pairs(days, focus)?
    };
    for &(a, b, ips, at) in login.iter().flatten() {
        let e = out.entry(pair(a, b)).or_default();
        e.login_ips = ips;
        e.last_at = e.last_at.max(at);
    }
    let reviewed = q(
        &format!(
            "SELECT user_a, user_b, verdict, note, {} FROM account_links WHERE $1::bigint IS NULL OR user_a = $1 OR user_b = $1 LIMIT 5000",
            ms("reviewed_at")
        ),
        &[db::oi(focus)],
    )?;
    for r in &reviewed {
        let e = out.entry((r.n(0), r.n(1))).or_default();
        e.verdict = Some(r.s(2));
        e.note = r.os(3);
        e.reviewed_at = r.on(4);
    }
    Ok((out, login.is_some()))
}

fn swarm_edges(days: i64, focus: Option<i64>, out: &mut Graph) -> R<()> {
    let rows = q(
        &format!(
            "SELECT e.user_id, e.torrent_id, e.details->'counterparty_user_ids', {} FROM evidence e
              WHERE e.kind = 'anticheat.shared_ip_counterparty'
                AND e.verdict IS DISTINCT FROM 'false_positive'
                AND e.occurred_at > now() - make_interval(days => $1::int)
                AND ($2::bigint IS NULL OR e.user_id = $2 OR e.details->'counterparty_user_ids' @> jsonb_build_array($2::bigint))
              ORDER BY e.occurred_at DESC LIMIT 20000",
            ms("e.occurred_at")
        ),
        &[i(days), db::oi(focus)],
    )?;
    for r in &rows {
        let (u, tid, at) = (r.n(0), r.on(1), r.n(3));
        for o in r.v(2).as_array().into_iter().flatten().filter_map(Value::as_i64) {
            if o == u || focus.is_some_and(|f| f != u && f != o) {
                continue;
            }
            let e = out.entry(pair(u, o)).or_default();
            e.torrents.extend(tid);
            e.hits += 1;
            e.last_at = e.last_at.max(at);
        }
    }
    Ok(())
}

/// 连通分量（见 `linked`）；簇内成员升序，簇按成员数、边上种子数 + 共用登录 IP 数降序
pub fn clusters(edges: &BTreeMap<(i64, i64), Edge>, min_torrents: usize) -> Vec<Vec<i64>> {
    fn find(p: &mut HashMap<i64, i64>, x: i64) -> i64 {
        let up = *p.entry(x).or_insert(x);
        if up == x {
            return x;
        }
        let root = find(p, up);
        p.insert(x, root);
        root
    }
    let mut parent: HashMap<i64, i64> = HashMap::new();
    let mut weight: HashMap<(i64, i64), usize> = HashMap::new();
    for (&(a, b), e) in edges {
        if linked(e, min_torrents) {
            let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
            if ra != rb {
                parent.insert(ra.max(rb), ra.min(rb));
            }
            weight.insert(
                (a, b),
                e.torrents.len() + usize::try_from(e.login_ips).unwrap_or(0),
            );
        }
    }
    let mut groups: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
    let nodes: Vec<i64> = parent.keys().copied().collect();
    for n in nodes {
        let r = find(&mut parent, n);
        groups.entry(r).or_default().push(n);
    }
    let mut out: Vec<(usize, Vec<i64>)> = groups
        .into_values()
        .map(|mut g| {
            g.sort_unstable();
            let w = weight
                .iter()
                .filter(|((a, _), _)| g.binary_search(a).is_ok())
                .map(|(_, w)| *w)
                .sum();
            (w, g)
        })
        .collect();
    out.sort_by(|(wa, a), (wb, b)| b.len().cmp(&a.len()).then(wb.cmp(wa)).then(a[0].cmp(&b[0])));
    out.into_iter().map(|(_, g)| g).collect()
}

fn members_json(ids: &[i64]) -> R<HashMap<i64, Value>> {
    let names = usernames(ids)?;
    let risk: HashMap<i64, (i64, String, Option<i64>)> = q(
        "SELECT user_id, score, band, open_case_id FROM user_risk WHERE user_id = ANY($1)",
        &[ints(ids.to_vec())],
    )?
    .iter()
    .map(|r| (r.n(0), (r.n(1), r.s(2), r.on(3))))
    .collect();
    Ok(ids
        .iter()
        .map(|id| {
            let (score, band, case) = risk.get(id).cloned().unwrap_or((0, "none".into(), None));
            (*id, json!({ "id": id, "username": names.get(id), "score": score, "band": band, "open_case_id": case }))
        })
        .collect())
}

fn edge_json(a: i64, b: i64, e: &Edge) -> Value {
    json!({
        "a": a, "b": b,
        "torrent_count": e.torrents.len(),
        "torrent_ids": e.torrents.iter().take(20).collect::<Vec<_>>(),
        "hits": e.hits,
        "login_ips": e.login_ips,
        "last_at": (e.last_at > 0).then(|| db::iso(e.last_at)),
        "verdict": e.verdict,
        "note": e.note,
        "reviewed_at": db::iso_opt(e.reviewed_at),
    })
}

/// 后台“关联账号”标签页：账号簇列表
pub fn list(days: i64, min_torrents: i64, source: &str, limit: i64) -> R<Value> {
    let (all, login) = edges(days, None, source)?;
    let min = usize::try_from(min_torrents.clamp(1, 100)).unwrap_or(1);
    let mut groups = clusters(&all, min);
    groups.truncate(usize::try_from(limit.clamp(1, 100)).unwrap_or(50));
    let ids: Vec<i64> = groups
        .iter()
        .flat_map(|g| g.iter().take(MAX_MEMBERS).copied())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let m = members_json(&ids)?;
    let items: Vec<Value> = groups
        .iter()
        .map(|g| {
            let es: Vec<Value> = all
                .iter()
                .filter(|((a, b), e)| {
                    linked(e, min) && g.binary_search(a).is_ok() && g.binary_search(b).is_ok()
                })
                .map(|((a, b), e)| edge_json(*a, *b, e))
                .collect();
            json!({
                "size": g.len(),
                "members": g.iter().take(MAX_MEMBERS).map(|id| m[id].clone()).collect::<Vec<_>>(),
                "edges": es.iter().take(MAX_EDGES).collect::<Vec<_>>(),
                "confirmed": es.iter().filter(|e| e["verdict"] == "same_person").count(),
                "max_score": g.iter().take(MAX_MEMBERS).map(|id| m[id]["score"].as_i64().unwrap_or(0)).max(),
                "truncated": g.len() > MAX_MEMBERS || es.len() > MAX_EDGES,
            })
        })
        .collect();
    Ok(json!({ "days": days, "min_torrents": min, "source": source, "login_ip": login, "items": items }))
}

/// 用户详情 / 案件工作台：与该用户直接相连的账号（含复核为“无关”的，便于撤销）
pub fn of_user(user: i64, days: i64) -> R<Value> {
    let (es, login) = edges(days, Some(user), "all")?;
    let mut list: Vec<(i64, &Edge)> = es
        .iter()
        .map(|((a, b), e)| (if *a == user { *b } else { *a }, e))
        .collect();
    list.sort_by_key(|(o, e)| {
        (
            e.verdict.as_deref() == Some("unrelated"),
            e.verdict.as_deref() != Some("same_person"),
            std::cmp::Reverse(e.torrents.len() + usize::try_from(e.login_ips).unwrap_or(0)),
            *o,
        )
    });
    list.truncate(100);
    let ids: Vec<i64> = list.iter().map(|(o, _)| *o).collect();
    let m = members_json(&ids)?;
    let cluster = clusters(&edges(days, None, "all")?.0, 1)
        .into_iter()
        .find(|g| g.binary_search(&user).is_ok())
        .map_or(1, |g| g.len());
    Ok(json!({
        "user_id": user,
        "days": days,
        "cluster_size": cluster,
        "login_ip": login,
        "items": list.iter().map(|(o, e)| {
            let mut v = edge_json(user.min(*o), user.max(*o), e);
            v["user"] = m[o].clone();
            v
        }).collect::<Vec<_>>(),
    }))
}

/// 复核账号对：same_person / unrelated（覆盖之前的结论）
pub fn review(a: i64, b: i64, verdict: &str, note: Option<&str>, actor: i64) -> R<Value> {
    if a == b || a <= 0 || b <= 0 {
        return Err(db::validation("two different users required"));
    }
    if !VERDICTS.contains(&verdict) {
        return Err(db::validation("verdict must be same_person or unrelated"));
    }
    let note = note
        .map(|n| db::clean_text(n, 500, true))
        .transpose()?
        .filter(|n| !n.is_empty());
    if usernames(&[a, b])?.len() != 2 {
        return Err(err("kernel.not_found", "user not found"));
    }
    let (ua, ub) = pair(a, b);
    let r = q1(
        &format!(
            "INSERT INTO account_links (user_a, user_b, verdict, note, reviewed_by, reviewed_at) VALUES ($1, $2, $3, $4, $5, now())
             ON CONFLICT (user_a, user_b) DO UPDATE SET verdict = $3, note = $4, reviewed_by = $5, reviewed_at = now()
             RETURNING {}",
            ms("reviewed_at")
        ),
        &[i(ua), i(ub), t(verdict), db::ot(note.as_deref()), i(actor)],
    )?
    .ok_or_else(|| err("kernel.internal", "account link upsert"))?;
    db::audit(
        "anticheat.account_link_reviewed",
        &format!("user:{ua}"),
        &json!({ "other_user_id": ub, "verdict": verdict, "note": note }),
    )?;
    sync_evidence(ua, ub, verdict == "same_person", actor)?;
    Ok(json!({ "a": ua, "b": ub, "verdict": verdict, "note": note, "reviewed_at": db::iso(r.n(0)) }))
}

/// 撤销复核结论
pub fn clear(a: i64, b: i64) -> R<()> {
    let (ua, ub) = pair(a, b);
    if x(
        "DELETE FROM account_links WHERE user_a = $1 AND user_b = $2",
        &[i(ua), i(ub)],
    )? > 0
    {
        db::audit(
            "anticheat.account_link_cleared",
            &format!("user:{ua}"),
            &json!({ "other_user_id": ub }),
        )?;
        sync_evidence(ua, ub, false, 0)?;
    }
    Ok(())
}

/// “同一人”→ 两人各一条有效证据（已有则不重复写）；否则把这对的证据标为误判。之后重算两人
fn sync_evidence(a: i64, b: i64, same: bool, actor: i64) -> R<()> {
    for (u, o) in [(a, b), (b, a)] {
        let ext = format!("same:{a}:{b}:{u}");
        if same {
            if q1(
                "SELECT 1 FROM evidence WHERE source = 'anticheat' AND external_ref = $1 AND verdict IS DISTINCT FROM 'false_positive' LIMIT 1",
                &[t(&ext)],
            )?
            .is_none()
            {
                x(
                    "INSERT INTO evidence (occurred_at, user_id, source, kind, severity, details, external_ref, created_by)
                     VALUES (now(), $1, 'anticheat', $2, 'high', $3, $4, $5) ON CONFLICT DO NOTHING",
                    &[i(u), t(SAME_PERSON_KIND), j(&json!({ "counterparty_user_ids": [o] })), t(&ext), i(actor)],
                )?;
            }
        } else {
            x(
                "UPDATE evidence SET verdict = 'false_positive' WHERE source = 'anticheat' AND external_ref = $1 AND verdict IS DISTINCT FROM 'false_positive'",
                &[t(&ext)],
            )?;
        }
        core::rescore_user(u, Trigger::Manual, false)?;
    }
    Ok(())
}

/// 与 `user` 复核为“无关”的账号（同网段证据跳过）
pub fn unrelated_to(user: i64) -> R<HashSet<i64>> {
    Ok(q(
        "SELECT CASE WHEN user_a = $1 THEN user_b ELSE user_a END FROM account_links
          WHERE verdict = 'unrelated' AND (user_a = $1 OR user_b = $1)",
        &[i(user)],
    )?
    .iter()
    .map(|r| r.n(0))
    .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(torrents: &[i64], verdict: Option<&str>) -> Edge {
        Edge {
            torrents: torrents.iter().copied().collect(),
            hits: 1,
            verdict: verdict.map(str::to_owned),
            ..Edge::default()
        }
    }

    #[test]
    fn components() {
        let mut g = BTreeMap::new();
        g.insert((1, 2), e(&[10, 11], None));
        g.insert((2, 3), e(&[10], None));
        g.insert((4, 5), e(&[12], Some("unrelated")));
        g.insert((6, 7), e(&[], Some("same_person")));
        g.insert((8, 9), e(&[13], None));
        // 只共用过登录 IP：任何种子门槛下都连通
        g.insert(
            (10, 11),
            Edge {
                login_ips: 1,
                ..Edge::default()
            },
        );
        // 同样两人的簇按边上种子数排序
        assert_eq!(
            clusters(&g, 1),
            vec![vec![1, 2, 3], vec![8, 9], vec![10, 11], vec![6, 7]]
        );
        // 门槛 2 个种子：只剩 1–2、共用登录 IP 的 10–11 与人工确认的 6–7
        assert_eq!(clusters(&g, 2), vec![vec![1, 2], vec![10, 11], vec![6, 7]]);
        assert!(clusters(&BTreeMap::new(), 1).is_empty());
    }
}
