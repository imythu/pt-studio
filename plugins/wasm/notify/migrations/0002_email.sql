-- notify 外部渠道（LLD §1.2 / §1.4）：待投递记录（复制标题与正文，不依赖站内通知是否保存）；邮箱只在投递时读取，不落库
CREATE TABLE outbox (
    id               bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id          bigint NOT NULL,
    channel          text NOT NULL CHECK (channel IN ('email')),
    owner_plugin     text NOT NULL,
    template_key     text NOT NULL,
    category         text NOT NULL,
    title            text NOT NULL,
    body             text NOT NULL,
    link             text,
    idempotency_key  text,
    state            text NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'sent', 'failed', 'skipped')),
    attempts         integer NOT NULL DEFAULT 0,
    last_error       text,
    created_at       timestamptz NOT NULL DEFAULT now(),
    sent_at          timestamptz
);
CREATE INDEX outbox_user_idx ON outbox (user_id, channel, created_at DESC);
CREATE UNIQUE INDEX outbox_idem_idx ON outbox (owner_plugin, idempotency_key, channel) WHERE idempotency_key IS NOT NULL;
