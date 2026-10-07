//! 分布式租约（kernel/03 §9.3）。

use std::time::Duration;

use redis::{Script, aio::ConnectionManager};
use tokio::sync::watch;

const RENEW: &str = "if redis.call('GET', KEYS[1]) == ARGV[1] then return redis.call('PEXPIRE', KEYS[1], ARGV[2]) else return 0 end";
const RELEASE: &str =
    "if redis.call('GET', KEYS[1]) == ARGV[1] then return redis.call('DEL', KEYS[1]) else return 0 end";

/// 持有中的租约：后台续约，续约失败视为丢失（`lost` 置 true）。
#[derive(Debug)]
pub struct Lease {
    key: String,
    token: String,
    conn: ConnectionManager,
    lost: watch::Receiver<bool>,
    stop: watch::Sender<bool>,
}

impl Lease {
    pub async fn try_acquire(
        conn: &ConnectionManager,
        key: &str,
        node_id: &str,
        ttl: Duration,
    ) -> Option<Lease> {
        let mut c = conn.clone();
        let fence: i64 = redis::cmd("INCR")
            .arg(format!("k:fence:{key}"))
            .query_async(&mut c)
            .await
            .ok()?;
        let token = format!("{node_id}:{fence}");
        let ttl_ms = u64::try_from(ttl.as_millis()).unwrap_or(30_000);
        let ok: Option<String> = redis::cmd("SET")
            .arg(key)
            .arg(&token)
            .arg("NX")
            .arg("PX")
            .arg(ttl_ms)
            .query_async(&mut c)
            .await
            .ok()?;
        ok?;
        let (lost_tx, lost) = watch::channel(false);
        let (stop, mut stop_rx) = watch::channel(false);
        let (k, t, mut rc) = (key.to_owned(), token.clone(), conn.clone());
        tokio::spawn(async move {
            let script = Script::new(RENEW);
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(ttl / 3) => {}
                    _ = stop_rx.changed() => return,
                }
                let r: redis::RedisResult<i64> =
                    script.key(&k).arg(&t).arg(ttl_ms).invoke_async(&mut rc).await;
                if !matches!(r, Ok(1)) {
                    tracing::warn!(lease = %k, "lease lost");
                    let _ = lost_tx.send(true);
                    return;
                }
            }
        });
        Some(Lease {
            key: key.to_owned(),
            token,
            conn: conn.clone(),
            lost,
            stop,
        })
    }

    pub fn lost(&self) -> watch::Receiver<bool> {
        self.lost.clone()
    }

    pub fn is_lost(&self) -> bool {
        *self.lost.borrow()
    }

    pub async fn release(self) {
        let _ = self.stop.send(true);
        let mut c = self.conn.clone();
        let _: redis::RedisResult<i64> = Script::new(RELEASE)
            .key(&self.key)
            .arg(&self.token)
            .invoke_async(&mut c)
            .await;
    }
}
