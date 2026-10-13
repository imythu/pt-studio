-- ai-moderation（LLD §3）：标记（不存正文）
CREATE TABLE flags (
    id              bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    target          text NOT NULL UNIQUE,
    categories      jsonb NOT NULL,
    max_category    text NOT NULL,
    max_score       double precision NOT NULL,
    source          text NOT NULL CHECK (source IN ('local', 'model')),
    state           text NOT NULL CHECK (state IN ('reported', 'review', 'dismissed')),
    report_case_id  bigint,
    outcome         text CHECK (outcome IN ('true_positive', 'false_positive')),
    created_at      timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX flags_review_idx ON flags (created_at DESC) WHERE state = 'review';
