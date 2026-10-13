-- mcp（LLD §3）：写操作的站内确认；OAuth 表在 OAuth 实现时加入
CREATE TABLE confirmations (
    id           text PRIMARY KEY,
    user_id      bigint NOT NULL,
    token_id     text NOT NULL,
    client_id    text,
    tool         text NOT NULL,
    args         jsonb NOT NULL,
    args_hash    bytea NOT NULL,
    summary      text NOT NULL,
    state        text NOT NULL CHECK (state IN ('pending', 'approved', 'executed', 'rejected', 'expired')),
    result       jsonb,
    expires_at   timestamptz NOT NULL,
    created_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX confirmations_user_idx ON confirmations (user_id, created_at DESC) WHERE state = 'pending';
