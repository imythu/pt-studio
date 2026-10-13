-- 个人 API token 与 MCP token（03 §1.5、§3）；feed key 留待后续
CREATE TABLE tokens (
    id                  uuid PRIMARY KEY,
    user_id             bigint NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind                text NOT NULL CHECK (kind IN ('api', 'mcp', 'feed')),
    name                text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 64),
    scopes              text[] NOT NULL,
    token_hash          bytea NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    audience_plugin     text,
    issued_by           text NOT NULL,
    expires_at          timestamptz,
    last_used_at        timestamptz,
    created_at          timestamptz NOT NULL DEFAULT now(),
    revoked_at          timestamptz,
    CONSTRAINT tokens_feed_check CHECK ((kind = 'feed') = (audience_plugin IS NOT NULL))
);
CREATE INDEX tokens_user_idx ON tokens (user_id, kind) WHERE revoked_at IS NULL;
