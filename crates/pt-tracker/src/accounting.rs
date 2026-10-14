//! 增量计算（design/10 §5.3）：给定上一次状态与本次上报，计算 Δup / Δdown / Δseed / Δleech 与信号位。纯函数。
//! 原则：宁可少记，不能多记——拿不准时只建立基线（baseline_reset），异常都以信号位交给下游。

use serde::{Deserialize, Serialize};

/// (user, info_hash, peer_id) 会话上一次被接受的累计值
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerState {
    pub up: u64,
    pub down: u64,
    pub left: u64,
    /// 上一次 announce 的时间（unix 秒）
    pub ts: u64,
    /// 该会话是否已经上报过 completed（防止重复计完成）
    pub completed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Started,
    Stopped,
    Completed,
    None,
}

impl Event {
    pub fn parse(s: Option<&str>) -> Option<Self> {
        match s.unwrap_or("") {
            "started" => Some(Self::Started),
            "stopped" => Some(Self::Stopped),
            "completed" => Some(Self::Completed),
            "" | "empty" | "paused" => Some(Self::None),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Stopped => "stopped",
            Self::Completed => "completed",
            Self::None => "none",
        }
    }
}

/// 本次上报的原始累计值
#[derive(Debug, Clone, Copy)]
pub struct Report {
    pub up: u64,
    pub down: u64,
    pub left: u64,
    pub event: Event,
    pub now: u64,
}

/// 信号位（design/10 §5.4 flags）
pub const FIRST_SEEN: u32 = 1;
pub const BASELINE_RESET: u32 = 1 << 1;
pub const UPLOAD_REGRESSED: u32 = 1 << 2;
pub const DOWNLOAD_REGRESSED: u32 = 1 << 3;
pub const FIRST_COMPLETED: u32 = 1 << 4;
pub const SEED_TIME_CAPPED: u32 = 1 << 5;
/// 本段平均速度超过上限：这一段的流量不计（bit6 留给 left_exceeds_size）
pub const SPEED_EXCEEDED: u32 = 1 << 7;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Delta {
    pub up: u64,
    pub down: u64,
    pub seed: u64,
    pub leech: u64,
    pub flags: u32,
}

impl Delta {
    /// 没有流量、时长与信号位
    pub fn is_zero(&self) -> bool {
        self.up == 0 && self.down == 0 && self.seed == 0 && self.leech == 0 && self.flags == 0
    }
}

/// 返回 (增量, 新状态)；新状态为 `None` 表示删除（event = stopped）。
/// `max_rate`：上传 / 下载各自的平均速度上限（字节 / 秒），0 表示不限
pub fn compute(
    prev: Option<&PeerState>,
    r: &Report,
    max_interval: u64,
    max_rate: u64,
) -> (Delta, Option<PeerState>) {
    let mut d = Delta::default();
    match prev {
        None => {
            d.flags |= FIRST_SEEN;
            // 按 BEP 3，started 时的累计值属于本会话，正常为 0。非 0 无从核实（伪造一条 started 就能凭空记账），
            // 与状态丢失一样只建立基线
            if r.event != Event::Started || r.up != 0 || r.down != 0 {
                d.flags |= BASELINE_RESET;
            }
        }
        Some(p) => {
            if r.up >= p.up {
                d.up = r.up - p.up;
            } else {
                d.flags |= UPLOAD_REGRESSED;
            }
            if r.down >= p.down {
                d.down = r.down - p.down;
            } else {
                d.flags |= DOWNLOAD_REGRESSED;
            }
            let elapsed = r.now.saturating_sub(p.ts);
            let limit = max_rate.saturating_mul(elapsed.max(1));
            if max_rate > 0 && (d.up > limit || d.down > limit) {
                d.flags |= SPEED_EXCEEDED;
                d.up = 0;
                d.down = 0;
            }
            let counted = if elapsed > max_interval {
                d.flags |= SEED_TIME_CAPPED;
                max_interval
            } else {
                elapsed
            };
            // 时长记到上一次的角色上
            if p.left == 0 {
                d.seed = counted;
            } else {
                d.leech = counted;
            }
        }
    }
    let was_completed = prev.is_some_and(|p| p.completed);
    if r.event == Event::Completed && !was_completed {
        d.flags |= FIRST_COMPLETED;
    }
    let next = (r.event != Event::Stopped).then_some(PeerState {
        up: r.up,
        down: r.down,
        left: r.left,
        ts: r.now,
        completed: was_completed || r.event == Event::Completed,
    });
    (d, next)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rep(up: u64, down: u64, left: u64, event: Event, now: u64) -> Report {
        Report {
            up,
            down,
            left,
            event,
            now,
        }
    }

    #[test]
    fn fresh_start_and_progress() {
        let (d, s) = compute(None, &rep(0, 0, 100, Event::Started, 1000), 2700, 0);
        assert_eq!((d.up, d.down, d.flags), (0, 0, FIRST_SEEN));
        let s = s.unwrap();
        let (d, s2) = compute(Some(&s), &rep(10, 60, 40, Event::None, 1600), 2700, 0);
        assert_eq!((d.up, d.down, d.leech, d.seed), (10, 60, 600, 0));
        let (d, s3) = compute(s2.as_ref(), &rep(10, 100, 0, Event::Completed, 1700), 2700, 0);
        assert_eq!((d.down, d.flags), (40, FIRST_COMPLETED));
        assert!(s3.unwrap().completed);
        // 做种期间：时长记为做种
        let (d, _) = compute(s3.as_ref(), &rep(500, 100, 0, Event::None, 2700), 2700, 0);
        assert_eq!((d.up, d.seed, d.leech), (490, 1000, 0));
    }

    #[test]
    fn anomalies() {
        // 状态丢失且不是 started：只建基线
        let (d, s) = compute(None, &rep(999, 999, 0, Event::None, 10), 2700, 0);
        assert_eq!((d.up, d.down, d.flags), (0, 0, FIRST_SEEN | BASELINE_RESET));
        // 累计值回退
        let (d, _) = compute(s.as_ref(), &rep(5, 1000, 0, Event::None, 20), 2700, 0);
        assert_eq!(
            (d.up, d.down, d.flags & UPLOAD_REGRESSED),
            (0, 1, UPLOAD_REGRESSED)
        );
        // 长时间离线：时长截断
        let (d, _) = compute(s.as_ref(), &rep(999, 999, 0, Event::None, 100_000), 2700, 0);
        assert_eq!((d.seed, d.flags), (2700, SEED_TIME_CAPPED));
        // 重复 completed 不再计
        let done = PeerState {
            up: 0,
            down: 0,
            left: 0,
            ts: 0,
            completed: true,
        };
        let (d, _) = compute(Some(&done), &rep(0, 0, 0, Event::Completed, 1), 2700, 0);
        assert_eq!(d.flags & FIRST_COMPLETED, 0);
        // stopped：计入最后一段并删除状态
        let (d, s) = compute(Some(&done), &rep(7, 0, 0, Event::Stopped, 5), 2700, 0);
        assert_eq!((d.up, s), (7, None));
    }

    #[test]
    fn forged_reports() {
        // 首报 started 却带着累计值：只建基线，不计
        let (d, s) = compute(None, &rep(50 << 30, 0, 100, Event::Started, 1000), 2700, 0);
        assert_eq!((d.up, d.flags), (0, FIRST_SEEN | BASELINE_RESET));
        let s = s.unwrap();
        // 之后的正常增量照常计
        let (d, s2) = compute(
            Some(&s),
            &rep((50 << 30) + 1000, 0, 100, Event::None, 1900),
            2700,
            100,
        );
        assert_eq!((d.up, d.flags), (1000, 0));
        // 超过速度上限：这一段不计，基线前移
        let (d, s3) = compute(s2.as_ref(), &rep(60 << 30, 0, 100, Event::None, 2800), 2700, 100);
        assert_eq!((d.up, d.down, d.flags, d.leech), (0, 0, SPEED_EXCEEDED, 900));
        assert_eq!(s3.unwrap().up, 60 << 30);
        // 同一秒内的增量按 1 秒算
        let (d, _) = compute(
            s2.as_ref(),
            &rep((50 << 30) + 1100, 0, 100, Event::None, 1900),
            2700,
            100,
        );
        assert_eq!((d.up, d.flags), (100, 0));
    }
}
