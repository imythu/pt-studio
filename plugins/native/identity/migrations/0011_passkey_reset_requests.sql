-- ctx.users.reset_passkey 的幂等记录：同一调用方同一幂等键只重置一次，重放返回首次结果
CREATE TABLE passkey_reset_requests (
    source           text NOT NULL,
    idempotency_key  text NOT NULL CHECK (char_length(idempotency_key) BETWEEN 1 AND 200),
    user_id          bigint NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    version          integer NOT NULL,
    created_at       timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (source, idempotency_key)
);
