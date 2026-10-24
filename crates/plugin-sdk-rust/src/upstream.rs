//! 外部上游 API 的保护：每秒固定窗口限速与熔断，状态放在插件 Redis（`redis = true`；不在事务内，调用失败回滚也保留计数）。
//! Redis 不可用时放行（不因保护机制本身挡住请求）。
//!
//! ```ignore
//! const TMDB: Upstream = Upstream::new("tmdb", "metadata-tmdb.upstream_error");
//! TMDB.acquire()?;                 // 熔断打开或限速等待后仍超限 → 可重试错误
//! let r = http_client::send(&req);
//! TMDB.record(r.as_ref().is_ok_and(|r| r.status < 500 && r.status != 429));
//! ```

use std::time::Duration;

use crate::{PluginError, R, redis, time};

#[derive(Debug, Clone, Copy)]
pub struct Upstream {
    /// 键名片段：`rl:<name>:<秒>`、`cb:<name>:*`
    pub name: &'static str,
    /// 熔断或限速时返回的错误码
    pub error_code: &'static str,
    /// 每秒请求上限
    pub per_sec: i64,
    /// 连续失败多少次后打开熔断
    pub failures: i64,
    /// 熔断打开时长（秒）；到期后进入半开：下一次失败立即重新打开
    pub open_secs: i64,
}

impl Upstream {
    /// 默认：每秒 30 次、连续 10 次失败熔断 5 分钟
    pub const fn new(name: &'static str, error_code: &'static str) -> Self {
        Self {
            name,
            error_code,
            per_sec: 30,
            failures: 10,
            open_secs: 300,
        }
    }

    #[must_use]
    pub const fn per_sec(mut self, n: i64) -> Self {
        self.per_sec = n;
        self
    }

    #[must_use]
    pub const fn breaker(mut self, failures: i64, open_secs: i64) -> Self {
        self.failures = failures;
        self.open_secs = open_secs;
        self
    }

    fn err(&self, msg: &str) -> PluginError {
        PluginError {
            code: self.error_code.into(),
            message: msg.into(),
            details: None,
            retryable: true,
        }
    }

    /// 熔断是否打开
    pub fn is_open(&self) -> bool {
        matches!(
            cmd("EXISTS", &[&format!("cb:{}:open", self.name)]),
            Some(redis::RedisReply::Int(1))
        )
    }

    /// 发请求前调用：熔断打开时立即失败；本秒已满时等到下一秒（最多 2 次）
    pub fn acquire(&self) -> R<()> {
        if self.is_open() {
            return Err(self.err(&format!(
                "{} is temporarily unavailable (circuit open)",
                self.name
            )));
        }
        for _ in 0..3 {
            let now = time::now();
            let sec = now / 1000;
            let key = format!("rl:{}:{sec}", self.name);
            let Some(redis::RedisReply::Int(n)) = cmd("INCR", &[&key]) else {
                return Ok(());
            };
            if n == 1 {
                cmd("EXPIRE", &[&key, "2"]);
            }
            if n <= self.per_sec {
                return Ok(());
            }
            let wait = 1000 - now.rem_euclid(1000);
            std::thread::sleep(Duration::from_millis(u64::try_from(wait).unwrap_or(1000)));
        }
        Err(self.err(&format!("{} rate limit reached", self.name)))
    }

    /// 请求结束后调用：成功清零失败计数；失败累计，达到阈值（或半开期间）打开熔断
    pub fn record(&self, ok: bool) {
        let (fails, open, half) = (
            format!("cb:{}:fails", self.name),
            format!("cb:{}:open", self.name),
            format!("cb:{}:half", self.name),
        );
        if ok {
            cmd("DEL", &[&fails, &half]);
            return;
        }
        let half_open = matches!(cmd("EXISTS", &[&half]), Some(redis::RedisReply::Int(1)));
        let n = match cmd("INCR", &[&fails]) {
            Some(redis::RedisReply::Int(n)) => n,
            _ => return,
        };
        cmd("EXPIRE", &[&fails, "600"]);
        if half_open || n >= self.failures {
            let secs = self.open_secs.to_string();
            cmd("SET", &[&open, "1", "EX", &secs]);
            // 半开窗口：打开期结束后一段时间内再失败一次就重新打开
            let half_secs = (self.open_secs * 4).to_string();
            cmd("SET", &[&half, "1", "EX", &half_secs]);
            cmd("DEL", &[&fails]);
        }
    }
}

/// 429 / 503 的 `Retry-After`（秒数形式；日期形式或缺省按 1 秒），最多等 `cap` 秒
pub fn retry_after(value: Option<&str>, cap: u64) -> Duration {
    let secs = value.and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(1);
    Duration::from_secs(secs.clamp(1, cap.max(1)))
}

fn cmd(name: &str, args: &[&str]) -> Option<redis::RedisReply> {
    redis::call(&redis::Command {
        name: name.into(),
        args: args.iter().map(|a| a.as_bytes().to_vec()).collect(),
    })
    .ok()
}
