-- ai-upload-review（LLD §3）：审核报告，每个种子版本一份（重审覆盖）
CREATE TABLE reports (
    id             bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    torrent_id     bigint NOT NULL,
    revision       integer NOT NULL,
    verdict        text NOT NULL CHECK (verdict IN ('pass', 'warn', 'fail')),
    confidence     double precision,
    findings       jsonb NOT NULL,
    model          text,
    model_skipped  text,
    usage          jsonb,
    feedback       text CHECK (feedback IN ('useful', 'false_positive')),
    feedback_by    bigint,
    created_at     timestamptz NOT NULL DEFAULT now(),
    UNIQUE (torrent_id, revision)
);
