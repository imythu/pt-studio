-- notify（LLD §3 的站内部分）：通知（模板键 + 参数，前端按术语渲染；title / body 为回退文本）与按类别的偏好
CREATE TABLE notifications (
    id               bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id          bigint NOT NULL,
    owner_plugin     text NOT NULL,
    template_key     text NOT NULL,
    category         text NOT NULL,
    args             jsonb NOT NULL,
    title            text NOT NULL,
    body             text NOT NULL,
    link             text,
    idempotency_key  text,
    read_at          timestamptz,
    created_at       timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX notifications_user_idx ON notifications (user_id, id DESC);
CREATE INDEX notifications_unread_idx ON notifications (user_id) WHERE read_at IS NULL;
CREATE UNIQUE INDEX notifications_idem_idx ON notifications (owner_plugin, idempotency_key) WHERE idempotency_key IS NOT NULL;

CREATE TABLE prefs (
    user_id   bigint NOT NULL,
    category  text NOT NULL,
    channel   text NOT NULL,
    enabled   boolean NOT NULL,
    PRIMARY KEY (user_id, category, channel)
);
