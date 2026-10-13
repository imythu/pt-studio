-- mcp OAuth（LLD §3）：动态注册的客户端、授权码（只存哈希、一次性）、授权记录（刷新令牌哈希与当前访问令牌）
CREATE TABLE oauth_clients (
    client_id          text PRIMARY KEY,
    client_secret_hash bytea,
    client_name        text NOT NULL,
    redirect_uris      text[] NOT NULL,
    application_type   text NOT NULL CHECK (application_type IN ('web', 'native')),
    auth_method        text NOT NULL,
    created_at         timestamptz NOT NULL DEFAULT now(),
    last_used_at       timestamptz
);

CREATE TABLE auth_codes (
    code_hash       bytea PRIMARY KEY,
    client_id       text NOT NULL REFERENCES oauth_clients (client_id) ON DELETE CASCADE,
    user_id         bigint NOT NULL,
    scopes          text[] NOT NULL,
    redirect_uri    text NOT NULL,
    code_challenge  text NOT NULL,
    expires_at      timestamptz NOT NULL,
    used            boolean NOT NULL DEFAULT false
);

CREATE TABLE grants (
    user_id            bigint NOT NULL,
    client_id          text NOT NULL REFERENCES oauth_clients (client_id) ON DELETE CASCADE,
    scopes             text[] NOT NULL,
    refresh_hash       bytea NOT NULL UNIQUE,
    refresh_expires_at timestamptz NOT NULL,
    access_token_id    text,
    created_at         timestamptz NOT NULL DEFAULT now(),
    last_used_at       timestamptz,
    PRIMARY KEY (user_id, client_id)
);
