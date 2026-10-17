-- 邮件挑战（identity 03 §1.8）：邮箱验证、找回密码。令牌 = HMAC(challenge-key, id ‖ nonce)，库中只存其 SHA-256。
-- 投递由 notify 订阅 user.challenge_issued 后经 ctx.auth.challenge_delivery 取收件人与链接（链接不进事件、不进通知库）。
CREATE TABLE challenges (
    id                 uuid PRIMARY KEY,
    user_id            bigint NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    purpose            text NOT NULL CHECK (purpose IN ('email_verify', 'password_reset')),
    nonce              bytea NOT NULL,
    token_hash         bytea NOT NULL CHECK (octet_length(token_hash) = 32),
    expires_at         timestamptz NOT NULL,
    delivery_count     smallint NOT NULL DEFAULT 0,
    last_delivery_key  text,
    consumed_at        timestamptz,
    superseded_at      timestamptz,
    created_at         timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX challenges_user_purpose_idx ON challenges (user_id, purpose) WHERE consumed_at IS NULL AND superseded_at IS NULL;
CREATE INDEX challenges_expires_idx ON challenges (expires_at);
