//! 盒子标记（seedbox 插件登记、管理员批准的网段）：记账流中带有 peer IP，写入 `peers` 时按用户的网段判断 `is_seedbox`。
//!
//! 网段来自 `seedbox.v_ranges`（seedbox 插件未安装或未授权时视为没有网段），每分钟重载一次；IP 只在内存中比对，不落库。

use std::{collections::HashMap, net::IpAddr};

use pt_plugin_api::{Ctx, PResult};
use sqlx::Row;

#[derive(Debug, Default)]
pub struct Ranges(HashMap<u64, Vec<(IpAddr, u8)>>);

/// `net/bits` 是否包含 `ip`（IPv4-mapped IPv6 按 IPv4 比较）
pub fn cidr_contains(net: &IpAddr, bits: u8, ip: &IpAddr) -> bool {
    match (net.to_canonical(), ip.to_canonical()) {
        (IpAddr::V4(n), IpAddr::V4(a)) => {
            let b = u32::from(bits.min(32));
            let mask = if b == 0 { 0 } else { u32::MAX << (32 - b) };
            u32::from(n) & mask == u32::from(a) & mask
        }
        (IpAddr::V6(n), IpAddr::V6(a)) => {
            let b = u32::from(bits.min(128));
            let mask = if b == 0 { 0 } else { u128::MAX << (128 - b) };
            u128::from(n) & mask == u128::from(a) & mask
        }
        _ => false,
    }
}

impl Ranges {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn contains(&self, uid: u64, ip: &IpAddr) -> bool {
        self.0
            .get(&uid)
            .is_some_and(|list| list.iter().any(|(n, b)| cidr_contains(n, *b, ip)))
    }

    /// 读取已批准的网段；视图不存在或无权读取时返回空（不让事务出错）
    pub async fn load(ctx: &Ctx) -> PResult<Self> {
        let mut c = ctx.sql.conn().await?;
        // 按 oid 检查（按名字解析在没有 schema 权限时会报错并中止事务）
        let readable: bool = sqlx::query_scalar(
            "SELECT coalesce((SELECT has_schema_privilege(current_user, n.oid, 'USAGE') AND has_table_privilege(current_user, c.oid, 'SELECT')
                                FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
                               WHERE n.nspname = 'seedbox' AND c.relname = 'v_ranges'), false)",
        )
        .fetch_one(&mut *c)
        .await?;
        if !readable {
            return Ok(Self::default());
        }
        let rows = sqlx::query("SELECT user_id, host(cidr), masklen(cidr) FROM seedbox.v_ranges")
            .fetch_all(&mut *c)
            .await?;
        let mut m: HashMap<u64, Vec<(IpAddr, u8)>> = HashMap::new();
        for r in rows {
            let (Ok(uid), Ok(ip)) = (u64::try_from(r.get::<i64, _>(0)), r.get::<String, _>(1).parse::<IpAddr>()) else {
                continue;
            };
            let bits = u8::try_from(r.get::<i32, _>(2)).unwrap_or(128);
            m.entry(uid).or_default().push((ip, bits));
        }
        Ok(Self(m))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cidrs() {
        let ip = |s: &str| s.parse::<IpAddr>().unwrap_or(IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED));
        assert!(cidr_contains(&ip("203.0.113.0"), 24, &ip("203.0.113.77")));
        assert!(!cidr_contains(&ip("203.0.113.0"), 24, &ip("203.0.114.1")));
        assert!(cidr_contains(&ip("203.0.113.0"), 24, &ip("::ffff:203.0.113.9")));
        assert!(cidr_contains(&ip("2001:db8:1::"), 48, &ip("2001:db8:1:ffff::1")));
        assert!(!cidr_contains(&ip("2001:db8:1::"), 48, &ip("2001:db8:2::1")));
        assert!(!cidr_contains(&ip("2001:db8:1::"), 48, &ip("203.0.113.9")));
    }
}
