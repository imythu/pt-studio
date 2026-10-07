//! Redis 网关（kernel/03 §6.5）：命令白名单、键改写为 `p:<plugin_id>:<key>`。

use std::collections::BTreeMap;

use async_trait::async_trait;
use pt_plugin_api::{PResult, PluginError, RedisCommand, RedisReply, host::Redis};
use redis::{Value, aio::ConnectionManager};

/// 键参数的位置规则。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Keys {
    /// 第一个参数是键
    First,
    /// 前两个参数是键
    FirstTwo,
    /// 全部参数都是键
    All,
    /// 键、值交替（MSET）
    Alternate,
    /// `dest numkeys key...`（ZUNIONSTORE 等），dest 也是键
    DestNumkeys,
    /// `numkeys key...`（SINTERCARD）
    Numkeys,
    /// `op dest key...`（BITOP）
    SkipFirst,
    /// SCAN：强制 MATCH 前缀
    Scan,
}

fn rule(cmd: &str) -> Option<Keys> {
    use Keys::{All, Alternate, DestNumkeys, First, FirstTwo, Numkeys, Scan, SkipFirst};
    Some(match cmd {
        "DEL" | "UNLINK" | "EXISTS" | "TOUCH" | "MGET" | "PFCOUNT" | "PFMERGE" | "SINTER" | "SUNION"
        | "SDIFF" | "SINTERSTORE" | "SUNIONSTORE" | "SDIFFSTORE" => All,
        "MSET" | "MSETNX" => Alternate,
        "RENAME" | "RENAMENX" | "COPY" | "SMOVE" | "LMOVE" | "ZRANGESTORE" => FirstTwo,
        "ZUNIONSTORE" | "ZINTERSTORE" => DestNumkeys,
        "SINTERCARD" => Numkeys,
        "BITOP" => SkipFirst,
        "SCAN" => Scan,
        "EXPIRE" | "PEXPIRE" | "EXPIREAT" | "PEXPIREAT" | "EXPIRETIME" | "TTL" | "PTTL" | "PERSIST"
        | "TYPE" | "GET" | "SET" | "SETNX" | "SETEX" | "PSETEX" | "GETSET" | "GETDEL" | "GETEX" | "INCR"
        | "INCRBY" | "INCRBYFLOAT" | "DECR" | "DECRBY" | "APPEND" | "STRLEN" | "GETRANGE" | "SETRANGE"
        | "SETBIT" | "GETBIT" | "BITCOUNT" | "BITPOS" | "BITFIELD" | "BITFIELD_RO" | "PFADD" | "HGET"
        | "HSET" | "HSETNX" | "HMGET" | "HMSET" | "HDEL" | "HEXISTS" | "HGETALL" | "HKEYS" | "HVALS"
        | "HLEN" | "HINCRBY" | "HINCRBYFLOAT" | "HSCAN" | "HSTRLEN" | "HRANDFIELD" | "HEXPIRE"
        | "HPEXPIRE" | "HTTL" | "HPERSIST" | "LPUSH" | "RPUSH" | "LPUSHX" | "RPUSHX" | "LPOP" | "RPOP"
        | "LLEN" | "LRANGE" | "LINDEX" | "LSET" | "LREM" | "LTRIM" | "LINSERT" | "LPOS" | "SADD" | "SREM"
        | "SMEMBERS" | "SISMEMBER" | "SMISMEMBER" | "SCARD" | "SPOP" | "SRANDMEMBER" | "SSCAN" | "ZADD"
        | "ZREM" | "ZSCORE" | "ZMSCORE" | "ZINCRBY" | "ZCARD" | "ZCOUNT" | "ZRANGE" | "ZRANK"
        | "ZREVRANK" | "ZREVRANGE" | "ZRANGEBYSCORE" | "ZREVRANGEBYSCORE" | "ZREMRANGEBYSCORE"
        | "ZREMRANGEBYRANK" | "ZPOPMIN" | "ZPOPMAX" | "ZSCAN" | "XADD" | "XRANGE" | "XREVRANGE" | "XLEN"
        | "XTRIM" | "XDEL" => First,
        _ => return None,
    })
}

const MAX_ARGS: usize = 1024;
const MAX_ARG_BYTES: usize = 1 << 20;
const MAX_PIPELINE: usize = 1000;

/// 改写一条插件命令；返回 (命令名, 改写后的参数)。
pub fn rewrite(prefix: &[u8], cmd: &RedisCommand) -> PResult<(String, Vec<Vec<u8>>)> {
    let name = cmd.name.to_ascii_uppercase();
    let r = rule(&name).ok_or_else(|| {
        PluginError::new(
            "kernel.redis_command_denied",
            format!("redis command `{name}` is not allowed"),
        )
    })?;
    if cmd.args.len() > MAX_ARGS || cmd.args.iter().any(|a| a.len() > MAX_ARG_BYTES) {
        return Err(PluginError::bad_request("redis command too large"));
    }
    let key = |k: &[u8]| -> PResult<Vec<u8>> {
        if k.is_empty() || k.len() > 512 {
            return Err(PluginError::bad_request("redis key must be 1..=512 bytes"));
        }
        let mut v = prefix.to_vec();
        v.extend_from_slice(k);
        Ok(v)
    };
    let mut args = cmd.args.clone();
    let len = args.len();
    let need = |n: usize| {
        if len < n {
            Err(PluginError::bad_request(format!("{name}: missing arguments")))
        } else {
            Ok(())
        }
    };
    match r {
        Keys::First => {
            need(1)?;
            args[0] = key(&args[0])?;
        }
        Keys::FirstTwo => {
            need(2)?;
            args[0] = key(&args[0])?;
            args[1] = key(&args[1])?;
            if name == "COPY" && args.iter().any(|a| a.eq_ignore_ascii_case(b"DB")) {
                return Err(PluginError::new(
                    "kernel.redis_command_denied",
                    "COPY ... DB is not allowed",
                ));
            }
        }
        Keys::All => {
            need(1)?;
            for a in &mut args {
                *a = key(a)?;
            }
        }
        Keys::Alternate => {
            need(2)?;
            for a in args.iter_mut().step_by(2) {
                *a = key(a)?;
            }
        }
        Keys::DestNumkeys | Keys::Numkeys => {
            let start = usize::from(r == Keys::DestNumkeys);
            need(start + 1)?;
            if start == 1 {
                args[0] = key(&args[0])?;
            }
            let n: usize = std::str::from_utf8(&args[start])
                .ok()
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| PluginError::bad_request("numkeys must be an integer"))?;
            need(start + 1 + n)?;
            for a in &mut args[start + 1..start + 1 + n] {
                *a = key(a)?;
            }
        }
        Keys::SkipFirst => {
            need(2)?;
            for a in &mut args[1..] {
                *a = key(a)?;
            }
        }
        Keys::Scan => {
            need(1)?;
            let mut out = vec![args[0].clone()];
            let mut pattern: Vec<u8> = b"*".to_vec();
            let mut i = 1;
            while i < args.len() {
                let opt = args[i].to_ascii_uppercase();
                let val = args
                    .get(i + 1)
                    .cloned()
                    .ok_or_else(|| PluginError::bad_request("SCAN option without value"))?;
                if opt == b"MATCH" {
                    pattern = val;
                } else {
                    out.push(args[i].clone());
                    out.push(val);
                }
                i += 2;
            }
            let mut p = prefix.to_vec();
            p.extend_from_slice(&pattern);
            out.push(b"MATCH".to_vec());
            out.push(p);
            args = out;
        }
    }
    Ok((name, args))
}

fn to_reply(v: Value) -> RedisReply {
    match v {
        Value::Nil => RedisReply::Nil,
        Value::Okay => RedisReply::Ok,
        Value::Int(i) => RedisReply::Int(i),
        Value::Double(d) => RedisReply::Double(d),
        Value::Boolean(b) => RedisReply::Int(i64::from(b)),
        Value::BulkString(b) => RedisReply::Bulk(b),
        Value::SimpleString(s) => RedisReply::Simple(s),
        Value::VerbatimString { text, .. } => RedisReply::Bulk(text.into_bytes()),
        Value::Array(a) | Value::Set(a) => RedisReply::Array(a.into_iter().map(to_reply).collect()),
        Value::Map(m) => RedisReply::Array(
            m.into_iter()
                .flat_map(|(k, v)| [to_reply(k), to_reply(v)])
                .collect(),
        ),
        Value::Attribute { data, .. } => to_reply(*data),
        other => RedisReply::Simple(format!("{other:?}")),
    }
}

/// SCAN 返回 [cursor, [keys...]]：去掉键名前缀。
fn strip_scan(reply: RedisReply, prefix: &[u8]) -> RedisReply {
    match reply {
        RedisReply::Array(mut a) if a.len() == 2 => {
            if let RedisReply::Array(keys) = &mut a[1] {
                for k in keys.iter_mut() {
                    if let RedisReply::Bulk(b) = k
                        && b.starts_with(prefix)
                    {
                        b.drain(..prefix.len());
                    }
                }
            }
            RedisReply::Array(a)
        }
        other => other,
    }
}

/// 每插件一个实例（`ctx.redis`）。
#[derive(Clone)]
pub struct RedisGateway {
    prefix: Vec<u8>,
    enabled: bool,
    main: Option<ConnectionManager>,
    /// 获授权的全局实例角色
    globals: BTreeMap<String, ConnectionManager>,
}

impl std::fmt::Debug for RedisGateway {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedisGateway")
            .field("prefix", &String::from_utf8_lossy(&self.prefix))
            .finish_non_exhaustive()
    }
}

impl RedisGateway {
    pub fn new(
        plugin_id: &str,
        enabled: bool,
        main: Option<ConnectionManager>,
        globals: BTreeMap<String, ConnectionManager>,
    ) -> Self {
        Self {
            prefix: format!("p:{plugin_id}:").into_bytes(),
            enabled,
            main,
            globals,
        }
    }

    fn conn(&self) -> PResult<ConnectionManager> {
        if !self.enabled {
            return Err(PluginError::capability_denied("capability `redis` not declared"));
        }
        self.main
            .clone()
            .ok_or_else(|| PluginError::unavailable("redis is not configured"))
    }
}

#[async_trait]
impl Redis for RedisGateway {
    async fn call(&self, cmd: RedisCommand) -> PResult<RedisReply> {
        let mut c = self.conn()?;
        let (name, args) = rewrite(&self.prefix, &cmd)?;
        let mut q = redis::cmd(&name);
        for a in &args {
            q.arg(a.as_slice());
        }
        let v: Value = q.query_async(&mut c).await?;
        let r = to_reply(v);
        Ok(if name == "SCAN" {
            strip_scan(r, &self.prefix)
        } else {
            r
        })
    }

    async fn pipeline(&self, cmds: Vec<RedisCommand>, atomic: bool) -> PResult<Vec<RedisReply>> {
        if cmds.len() > MAX_PIPELINE {
            return Err(PluginError::bad_request("pipeline too long"));
        }
        let mut c = self.conn()?;
        let mut p = redis::pipe();
        if atomic {
            p.atomic();
        }
        let mut names = Vec::with_capacity(cmds.len());
        for cmd in &cmds {
            let (name, args) = rewrite(&self.prefix, cmd)?;
            let q = p.cmd(&name);
            for a in &args {
                q.arg(a.as_slice());
            }
            names.push(name);
        }
        let vals: Vec<Value> = p.query_async(&mut c).await?;
        Ok(vals
            .into_iter()
            .zip(names)
            .map(|(v, n)| {
                let r = to_reply(v);
                if n == "SCAN" {
                    strip_scan(r, &self.prefix)
                } else {
                    r
                }
            })
            .collect())
    }

    fn global(&self, role: &str) -> PResult<ConnectionManager> {
        self.globals
            .get(role)
            .cloned()
            .ok_or_else(|| PluginError::capability_denied(format!("redis_global role `{role}` not granted")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(name: &str, args: &[&str]) -> RedisCommand {
        RedisCommand {
            name: name.into(),
            args: args.iter().map(|a| a.as_bytes().to_vec()).collect(),
        }
    }

    fn s(v: &[Vec<u8>]) -> Vec<String> {
        v.iter()
            .map(|a| String::from_utf8_lossy(a).into_owned())
            .collect()
    }

    #[test]
    fn rewrites_keys() -> PResult<()> {
        let p = b"p:bonus:";
        assert_eq!(
            s(&rewrite(p, &c("set", &["a", "1", "EX", "5"]))?.1),
            ["p:bonus:a", "1", "EX", "5"]
        );
        assert_eq!(
            s(&rewrite(p, &c("MSET", &["a", "1", "b", "2"]))?.1),
            ["p:bonus:a", "1", "p:bonus:b", "2"]
        );
        assert_eq!(
            s(&rewrite(p, &c("DEL", &["a", "b"]))?.1),
            ["p:bonus:a", "p:bonus:b"]
        );
        assert_eq!(
            s(&rewrite(p, &c("ZUNIONSTORE", &["d", "2", "a", "b", "WEIGHTS", "1", "2"]))?.1),
            ["p:bonus:d", "2", "p:bonus:a", "p:bonus:b", "WEIGHTS", "1", "2"]
        );
        assert_eq!(
            s(&rewrite(p, &c("SCAN", &["0", "COUNT", "10"]))?.1),
            ["0", "COUNT", "10", "MATCH", "p:bonus:*"]
        );
        assert_eq!(
            s(&rewrite(p, &c("scan", &["0", "match", "u:*"]))?.1),
            ["0", "MATCH", "p:bonus:u:*"]
        );
        Ok(())
    }

    #[test]
    fn denies_dangerous() {
        for n in [
            "EVAL",
            "KEYS",
            "FLUSHALL",
            "CONFIG",
            "PUBLISH",
            "XREADGROUP",
            "SORT",
            "SELECT",
        ] {
            let e = rewrite(b"p:x:", &c(n, &["a"])).err().map(|e| e.code);
            assert_eq!(e.as_deref(), Some("kernel.redis_command_denied"), "{n}");
        }
        assert!(rewrite(b"p:x:", &c("COPY", &["a", "b", "DB", "2"])).is_err());
        assert!(rewrite(b"p:x:", &c("GET", &[""])).is_err());
        assert!(rewrite(b"p:x:", &c("ZUNIONSTORE", &["d", "5", "a"])).is_err());
    }
}
