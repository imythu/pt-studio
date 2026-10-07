//! `ctx.ledger` 服务契约（ledger 03 §2.1；Native 消费方依赖本模块）。

use async_trait::async_trait;
use pt_plugin_api::{Ctx, PResult};
use serde::{Deserialize, Serialize};

pub const LEDGER: &str = "ctx.ledger";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrencyOptions {
    pub transferable: Option<bool>,
    pub fee_bps: Option<u32>,
    pub min_transfer: Option<i64>,
    pub daily_out_limit: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrencyDef {
    pub code: String,
    pub scale: u8,
    pub name_term: String,
    #[serde(default)]
    pub options: CurrencyOptions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrencyInfo {
    pub code: String,
    pub scale: u8,
    pub name_term: String,
    pub owner_plugin: String,
    pub transferable: bool,
    pub fee_bps: u32,
    pub min_transfer: i64,
    pub daily_out_limit: i64,
    pub disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferRequest {
    pub currency: String,
    /// None = 系统发行
    pub from_user: Option<u64>,
    /// None = 系统回收
    pub to_user: Option<u64>,
    /// > 0，最小单位
    pub amount: i64,
    /// 原因码 `<plugin>.<code>`，前缀须为调用方插件 ID
    pub reason: String,
    #[serde(default)]
    pub memo: String,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferResult {
    pub entry_id: String,
    pub from_balance_after: Option<i64>,
    pub to_balance_after: Option<i64>,
    pub fee: i64,
    pub duplicate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BalanceInfo {
    pub currency: String,
    pub scale: u8,
    pub balance: i64,
    pub held: i64,
    pub available: i64,
    pub locked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryItem {
    pub entry_id: String,
    pub currency: String,
    pub kind: String,
    pub amount: i64,
    pub balance_after: i64,
    pub reason: String,
    pub memo: Option<String>,
    pub counterparty_user_id: Option<u64>,
    pub caller_plugin: String,
    pub reversed: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryPage {
    pub items: Vec<HistoryItem>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldRequest {
    pub currency: String,
    pub user: u64,
    pub amount: i64,
    pub reason: String,
    pub expires_at: Option<i64>,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureRequest {
    pub hold_id: String,
    pub to_user: Option<u64>,
    pub amount: i64,
    pub reason: String,
    #[serde(default)]
    pub memo: String,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockRequest {
    pub user: u64,
    pub currency: Option<String>,
    pub reason: String,
    pub reference: String,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupplyInfo {
    pub currency: String,
    pub issued: i64,
    pub burned: i64,
    pub circulating: i64,
    pub held: i64,
}

#[async_trait]
pub trait LedgerService: Send + Sync {
    async fn register_currency(&self, ctx: &Ctx, def: CurrencyDef) -> PResult<()>;
    async fn currencies(&self, ctx: &Ctx) -> PResult<Vec<CurrencyInfo>>;
    async fn transfer(&self, ctx: &Ctx, req: TransferRequest) -> PResult<TransferResult>;
    async fn transfer_batch(
        &self,
        ctx: &Ctx,
        reqs: Vec<TransferRequest>,
    ) -> PResult<Vec<PResult<TransferResult>>>;
    async fn balance(&self, ctx: &Ctx, currency: &str, user: u64) -> PResult<i64>;
    async fn balances(&self, ctx: &Ctx, user: u64) -> PResult<Vec<BalanceInfo>>;
    async fn history(
        &self,
        ctx: &Ctx,
        currency: Option<String>,
        user: u64,
        cursor: Option<String>,
        limit: u32,
    ) -> PResult<HistoryPage>;
    async fn reverse(
        &self,
        ctx: &Ctx,
        entry_id: &str,
        reason: &str,
        idempotency_key: &str,
        allow_negative: bool,
    ) -> PResult<String>;
    async fn hold(&self, ctx: &Ctx, req: HoldRequest) -> PResult<String>;
    async fn release_hold(&self, ctx: &Ctx, hold_id: &str, idempotency_key: &str) -> PResult<()>;
    async fn capture_hold(&self, ctx: &Ctx, req: CaptureRequest) -> PResult<TransferResult>;
    async fn lock_account(&self, ctx: &Ctx, req: LockRequest) -> PResult<String>;
    async fn unlock_account(&self, ctx: &Ctx, lock_id: &str, idempotency_key: &str) -> PResult<()>;
    async fn supply(&self, ctx: &Ctx, currency: &str) -> PResult<SupplyInfo>;
}
