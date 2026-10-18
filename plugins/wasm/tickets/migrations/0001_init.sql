-- 工单（联系管理组）：open = 等待员工处理，pending_user = 等待用户回复，closed = 已关闭
CREATE TABLE tickets (
    id            bigserial PRIMARY KEY,
    user_id       bigint NOT NULL,
    category      text NOT NULL CHECK (category IN ('account', 'torrent', 'appeal', 'suggestion', 'other')),
    subject       text NOT NULL CHECK (char_length(subject) BETWEEN 1 AND 200),
    state         text NOT NULL DEFAULT 'open' CHECK (state IN ('open', 'pending_user', 'closed')),
    assignee_id   bigint,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    closed_at     timestamptz,
    closed_by     bigint
);
CREATE INDEX tickets_user_idx ON tickets (user_id, id DESC);
CREATE INDEX tickets_queue_idx ON tickets (state, updated_at);

-- 消息：internal = 员工内部备注（用户看不到）
CREATE TABLE ticket_messages (
    id         bigserial PRIMARY KEY,
    ticket_id  bigint NOT NULL REFERENCES tickets (id) ON DELETE CASCADE,
    author_id  bigint NOT NULL,
    staff      boolean NOT NULL DEFAULT false,
    internal   boolean NOT NULL DEFAULT false,
    body       text NOT NULL,
    body_html  text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ticket_messages_ticket_idx ON ticket_messages (ticket_id, id);
