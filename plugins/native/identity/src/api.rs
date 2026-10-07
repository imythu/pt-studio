//! `ctx.users` 服务契约（catalog/services.md；Native 消费方依赖本模块）。

use async_trait::async_trait;
use pt_plugin_api::{Ctx, PResult};
use serde::{Deserialize, Serialize};

pub const USERS: &str = "ctx.users";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserSummary {
    pub id: u64,
    pub username: String,
    pub class_id: u32,
    pub status: String,
    pub title: Option<String>,
    pub avatar_url: Option<String>,
    /// Unix 毫秒
    pub created_at: i64,
}

#[async_trait]
pub trait UsersService: Send + Sync {
    async fn get(&self, ctx: &Ctx, id: u64) -> PResult<Option<UserSummary>>;
    /// 最多 1000 个；不存在的 ID 不出现在结果中
    async fn get_many(&self, ctx: &Ctx, ids: &[u64]) -> PResult<Vec<UserSummary>>;
    /// 下载种子时注入 announce URL 的 passkey。作用域 `read:passkey`（torrent、tracker-bridge）。
    async fn passkey_of(&self, ctx: &Ctx, id: u64) -> PResult<Option<String>>;
}
