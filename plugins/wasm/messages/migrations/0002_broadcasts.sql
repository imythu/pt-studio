-- 群发公告：只存一行，用户维护已读游标（不为每个用户复制）
CREATE TABLE broadcasts (
    id              bigserial PRIMARY KEY,
    subject         text NOT NULL CHECK (char_length(subject) BETWEEN 1 AND 200),
    body            text NOT NULL,
    body_html       text NOT NULL,
    markup          text NOT NULL DEFAULT 'markdown',
    min_class       integer,
    created_by      bigint,
    source_plugin   text,
    idempotency_key text,
    created_at      timestamptz NOT NULL DEFAULT now(),
    expires_at      timestamptz,
    revoked_at      timestamptz,
    UNIQUE (source_plugin, idempotency_key)
);

CREATE TABLE broadcast_cursors (
    user_id   bigint PRIMARY KEY,
    last_read bigint NOT NULL DEFAULT 0
);

CREATE TABLE broadcast_hidden (
    user_id      bigint NOT NULL,
    broadcast_id bigint NOT NULL REFERENCES broadcasts (id) ON DELETE CASCADE,
    PRIMARY KEY (user_id, broadcast_id)
);
