-- ai-assistant（LLD §3）：规则文档与段落、会话与问答（llm_messages 为本轮追加到历史的消息，下一轮原样回放）
CREATE TABLE docs (
    id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    title       text NOT NULL,
    body_md     text NOT NULL,
    tags        text[] NOT NULL DEFAULT '{}',
    min_class   bigint,
    active      boolean NOT NULL DEFAULT true,
    updated_at  timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE chunks (
    id        text PRIMARY KEY,
    doc_id    bigint NOT NULL REFERENCES docs (id) ON DELETE CASCADE,
    heading   text NOT NULL,
    body      text NOT NULL
);
CREATE TABLE sessions (
    id          text PRIMARY KEY,
    user_id     bigint NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX sessions_user_idx ON sessions (user_id, updated_at DESC);
CREATE TABLE messages (
    id            bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    session_id    text NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
    question      text NOT NULL,
    answer        text NOT NULL,
    citations     jsonb NOT NULL,
    llm_messages  jsonb NOT NULL,
    tools_used    text[] NOT NULL DEFAULT '{}',
    feedback      text CHECK (feedback IN ('useful', 'not_useful')),
    created_at    timestamptz NOT NULL DEFAULT now()
);
