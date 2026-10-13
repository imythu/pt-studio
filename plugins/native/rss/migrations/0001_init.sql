-- rss（LLD §3）：订阅定义；feed key 本身由 identity 签发与校验，这里只记录每个用户当前的 key（token ID 与前 8 位）
CREATE TABLE keys (
    user_id     bigint PRIMARY KEY,
    token_id    uuid NOT NULL,
    prefix      text NOT NULL CHECK (char_length(prefix) <= 16),
    created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE feeds (
    id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id     bigint NOT NULL,
    name        text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 60),
    definition  jsonb NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX feeds_user_idx ON feeds (user_id);
