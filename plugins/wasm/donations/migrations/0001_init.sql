-- 捐赠记录：由管理员在收到款项后登记（站点不接入支付）；amount 以最小货币单位（分）存储
CREATE TABLE donations (
    id          bigserial PRIMARY KEY,
    user_id     bigint NOT NULL,
    amount      bigint NOT NULL CHECK (amount > 0),
    method      text NOT NULL DEFAULT '' CHECK (char_length(method) <= 50),
    note        text NOT NULL DEFAULT '' CHECK (char_length(note) <= 500),
    public      boolean NOT NULL DEFAULT false,          -- 用户同意出现在公开捐赠名单
    vip_days    integer NOT NULL DEFAULT 0 CHECK (vip_days BETWEEN 0 AND 3650),
    bonus       bigint NOT NULL DEFAULT 0 CHECK (bonus >= 0),
    medal_id    bigint,
    vip_until   timestamptz,                             -- 授予后的 VIP 到期时间（含顺延）
    created_by  bigint NOT NULL,
    revoked_at  timestamptz,
    created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX donations_user_idx ON donations (user_id, created_at DESC);
CREATE INDEX donations_time_idx ON donations (created_at DESC) WHERE revoked_at IS NULL;

-- 到期提醒去重：每个 VIP 到期时间只提醒一次
CREATE TABLE reminders (
    user_id  bigint NOT NULL,
    until    timestamptz NOT NULL,
    PRIMARY KEY (user_id, until)
);
