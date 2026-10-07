-- search-meilisearch（LLD §3.1）。本版本只建用到的表：多实例 oplog、做种数暂存、同义词管理、失败任务隔离后续迭代加入。
-- 哈希以十六进制文本保存（sha256）。

CREATE TABLE indexes (
    name              text PRIMARY KEY CHECK (name ~ '^[a-z][a-z0-9_]{1,63}$'),
    owner_plugin      text NOT NULL,
    audience          text NOT NULL CHECK (audience IN ('user','admin')),
    is_builtin        boolean NOT NULL DEFAULT false,
    live_uid          text NOT NULL,
    status            text NOT NULL CHECK (status IN ('waiting_owner','creating','ready','rebuilding','stale','disabled','degraded')),
    generation        integer NOT NULL DEFAULT 0,
    distinct_attr     text,
    merged_schema     jsonb NOT NULL DEFAULT '{}'::jsonb,
    settings_hash     text,
    shape_hash        text,
    applied_settings_hash text,
    applied_shape_hash text,
    dual_write_uid    text,
    conflicts         jsonb NOT NULL DEFAULT '[]'::jsonb,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now(),
    version           integer NOT NULL DEFAULT 1
);

CREATE SEQUENCE pending_ops_seq;

CREATE TABLE pending_ops (
    index_name        text NOT NULL,
    entity_id         bigint NOT NULL,
    op                text NOT NULL CHECK (op IN ('upsert','delete')),
    lane              text NOT NULL CHECK (lane IN ('normal','priority')),
    seq               bigint NOT NULL DEFAULT nextval('pending_ops_seq'),
    first_enqueued_at timestamptz NOT NULL DEFAULT now(),
    enqueued_at       timestamptz NOT NULL DEFAULT now(),
    source_event_id   uuid,
    PRIMARY KEY (index_name, entity_id)
);
CREATE INDEX pending_ops_lane_seq_idx ON pending_ops (index_name, lane, seq);
CREATE INDEX pending_ops_enqueued_at_idx ON pending_ops (enqueued_at);

CREATE TABLE indexed_docs (
    index_name   text NOT NULL,
    entity_id    bigint NOT NULL,
    doc_hash     text NOT NULL,
    group_id     bigint,
    author_id    bigint,
    partial      boolean NOT NULL DEFAULT false,
    indexed_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (index_name, entity_id)
) PARTITION BY HASH (entity_id);
CREATE TABLE indexed_docs_p00 PARTITION OF indexed_docs FOR VALUES WITH (MODULUS 16, REMAINDER 0);
CREATE TABLE indexed_docs_p01 PARTITION OF indexed_docs FOR VALUES WITH (MODULUS 16, REMAINDER 1);
CREATE TABLE indexed_docs_p02 PARTITION OF indexed_docs FOR VALUES WITH (MODULUS 16, REMAINDER 2);
CREATE TABLE indexed_docs_p03 PARTITION OF indexed_docs FOR VALUES WITH (MODULUS 16, REMAINDER 3);
CREATE TABLE indexed_docs_p04 PARTITION OF indexed_docs FOR VALUES WITH (MODULUS 16, REMAINDER 4);
CREATE TABLE indexed_docs_p05 PARTITION OF indexed_docs FOR VALUES WITH (MODULUS 16, REMAINDER 5);
CREATE TABLE indexed_docs_p06 PARTITION OF indexed_docs FOR VALUES WITH (MODULUS 16, REMAINDER 6);
CREATE TABLE indexed_docs_p07 PARTITION OF indexed_docs FOR VALUES WITH (MODULUS 16, REMAINDER 7);
CREATE TABLE indexed_docs_p08 PARTITION OF indexed_docs FOR VALUES WITH (MODULUS 16, REMAINDER 8);
CREATE TABLE indexed_docs_p09 PARTITION OF indexed_docs FOR VALUES WITH (MODULUS 16, REMAINDER 9);
CREATE TABLE indexed_docs_p10 PARTITION OF indexed_docs FOR VALUES WITH (MODULUS 16, REMAINDER 10);
CREATE TABLE indexed_docs_p11 PARTITION OF indexed_docs FOR VALUES WITH (MODULUS 16, REMAINDER 11);
CREATE TABLE indexed_docs_p12 PARTITION OF indexed_docs FOR VALUES WITH (MODULUS 16, REMAINDER 12);
CREATE TABLE indexed_docs_p13 PARTITION OF indexed_docs FOR VALUES WITH (MODULUS 16, REMAINDER 13);
CREATE TABLE indexed_docs_p14 PARTITION OF indexed_docs FOR VALUES WITH (MODULUS 16, REMAINDER 14);
CREATE TABLE indexed_docs_p15 PARTITION OF indexed_docs FOR VALUES WITH (MODULUS 16, REMAINDER 15);
CREATE INDEX indexed_docs_group_id_idx ON indexed_docs (group_id) WHERE group_id IS NOT NULL;

CREATE TABLE rebuilds (
    id             uuid PRIMARY KEY,
    index_name     text NOT NULL REFERENCES indexes(name),
    state          text NOT NULL CHECK (state IN ('preparing','building','replaying','verifying','swapping','cleanup','done','failed','cancelled')),
    trigger        text NOT NULL CHECK (trigger IN ('manual','auto','settings')),
    target_uid     text NOT NULL,
    from_generation integer NOT NULL,
    cursor_id      bigint NOT NULL DEFAULT 0,
    expected_total bigint,
    done_count     bigint NOT NULL DEFAULT 0,
    attempts       integer NOT NULL DEFAULT 0,
    requested_by   bigint,
    reason         text CHECK (char_length(reason) <= 500),
    error          text CHECK (char_length(error) <= 2000),
    started_at     timestamptz NOT NULL DEFAULT now(),
    finished_at    timestamptz,
    updated_at     timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX rebuilds_active_key ON rebuilds (index_name)
    WHERE state NOT IN ('done','failed','cancelled');

CREATE TABLE rebuild_touched (
    rebuild_id  uuid NOT NULL REFERENCES rebuilds(id) ON DELETE CASCADE,
    entity_id   bigint NOT NULL,
    PRIMARY KEY (rebuild_id, entity_id)
);

CREATE TABLE meili_tasks (
    instance_id  text NOT NULL,
    task_uid     bigint NOT NULL,
    index_uid    text NOT NULL,
    kind         text NOT NULL CHECK (kind IN ('docs','delete','stats','settings','swap','create','drop')),
    lane         text,
    index_name   text,
    entity_ids   bigint[] NOT NULL DEFAULT '{}',
    status       text NOT NULL DEFAULT 'enqueued' CHECK (status IN ('enqueued','succeeded','failed')),
    error_code   text,
    failed_count integer NOT NULL DEFAULT 0,
    enqueued_at  timestamptz NOT NULL DEFAULT now(),
    finished_at  timestamptz,
    PRIMARY KEY (instance_id, task_uid)
);
CREATE INDEX meili_tasks_status_idx ON meili_tasks (status, enqueued_at);
