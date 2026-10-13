-- reports（LLD §3）：工单（同一目标只有一个未结工单）、举报、理由目录（默认目录）、滥用计数
CREATE TABLE cases (
    id              bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    target          text NOT NULL,
    kind            text NOT NULL,
    state           text NOT NULL CHECK (state IN ('open', 'claimed', 'escalated', 'closed')),
    priority        integer NOT NULL DEFAULT 0,
    report_count    integer NOT NULL DEFAULT 0,
    claimed_by      bigint,
    claim_until     timestamptz,
    resolution      text,
    note_internal   text,
    note_public     text,
    action_results  jsonb,
    resolved_by     bigint,
    resolved_at     timestamptz,
    created_at      timestamptz NOT NULL DEFAULT now(),
    last_report_at  timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX cases_open_target_idx ON cases (target) WHERE state IN ('open', 'claimed', 'escalated');
CREATE INDEX cases_queue_idx ON cases (state, priority DESC, report_count DESC, created_at);

CREATE TABLE reports (
    id           bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    case_id      bigint NOT NULL REFERENCES cases (id),
    reporter_id  bigint,
    reason_code  text NOT NULL,
    note         text NOT NULL DEFAULT '',
    snapshot     jsonb,
    created_at   timestamptz NOT NULL DEFAULT now(),
    UNIQUE (case_id, reporter_id)
);
CREATE INDEX reports_reporter_idx ON reports (reporter_id, id DESC);

CREATE TABLE reasons (
    kind           text NOT NULL,
    code           text NOT NULL,
    priority       integer NOT NULL DEFAULT 20,
    requires_note  boolean NOT NULL DEFAULT false,
    active         boolean NOT NULL DEFAULT true,
    PRIMARY KEY (kind, code)
);
INSERT INTO reasons (kind, code, priority, requires_note) VALUES
    ('torrent', 'dupe', 20, false), ('torrent', 'trump', 20, false), ('torrent', 'bad_file', 50, false),
    ('torrent', 'wrong_info', 20, false), ('torrent', 'spam', 20, false), ('torrent', 'illegal', 100, false), ('torrent', 'other', 20, true),
    ('group', 'wrong_info', 20, false), ('group', 'other', 20, true),
    ('comment', 'spam', 20, false), ('comment', 'abuse', 50, false), ('comment', 'off_topic', 10, false),
    ('comment', 'illegal', 100, false), ('comment', 'personal_info', 60, false), ('comment', 'other', 20, true),
    ('user', 'spam', 20, false), ('user', 'abuse', 50, false), ('user', 'impersonation', 40, false),
    ('user', 'cheating', 60, false), ('user', 'other', 20, true);

CREATE TABLE abuse (
    user_id        bigint PRIMARY KEY,
    invalid_count  integer NOT NULL DEFAULT 0,
    window_start   timestamptz NOT NULL DEFAULT now(),
    banned_until   timestamptz
);
