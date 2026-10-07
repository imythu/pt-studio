-- 内核 schema（kernel/03 §16.1）。以 pt_gateway 身份执行（SET ROLE），对象属主为 pt_gateway。

CREATE TABLE kernel.plugin_registry (
    plugin_id       text PRIMARY KEY,
    version         text NOT NULL,
    kind            text NOT NULL CHECK (kind IN ('native','wasm')),
    manifest_sha256 text NOT NULL,
    first_seen_at   timestamptz NOT NULL DEFAULT now(),
    last_seen_at    timestamptz NOT NULL DEFAULT now(),
    removed_at      timestamptz
);

CREATE TABLE kernel.plugin_settings (
    plugin_id  text PRIMARY KEY,
    settings   jsonb NOT NULL DEFAULT '{}',
    version    bigint NOT NULL DEFAULT 1,
    updated_by bigint,
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE kernel.view_grant (
    consumer_plugin text NOT NULL,
    view_name       text NOT NULL,
    granted_at      timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (consumer_plugin, view_name)
);

-- 评审 R1 B-08：按日分区；分区由内核分区维护任务预建，DEFAULT 分区兜底
CREATE TABLE kernel.event_log (
    seq          bigint GENERATED ALWAYS AS IDENTITY,
    event_id     uuid NOT NULL,
    event_type   text NOT NULL,
    version      integer NOT NULL,
    source       text NOT NULL,
    key          text NOT NULL,
    occurred_at  timestamptz NOT NULL,
    actor        jsonb NOT NULL,
    payload      jsonb NOT NULL,
    trace        jsonb NOT NULL,
    PRIMARY KEY (occurred_at, seq)
) PARTITION BY RANGE (occurred_at);
CREATE TABLE kernel.event_log_default PARTITION OF kernel.event_log DEFAULT;
CREATE INDEX event_log_type_idx ON kernel.event_log (event_type, occurred_at);
CREATE INDEX event_log_event_id_idx ON kernel.event_log (event_id);

CREATE TABLE kernel.event_outbox (
    seq         bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    event_id    uuid NOT NULL,
    event_type  text NOT NULL,
    partition   integer NOT NULL,
    envelope    jsonb NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE kernel.event_dead_letter (
    id            bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    plugin_id     text NOT NULL,
    subscription  text NOT NULL,
    event_id      uuid NOT NULL,
    envelope      jsonb NOT NULL,
    attempts      integer NOT NULL,
    last_error    jsonb NOT NULL,
    state         text NOT NULL DEFAULT 'open' CHECK (state IN ('open','retried','ignored')),
    created_at    timestamptz NOT NULL DEFAULT now(),
    resolved_at   timestamptz,
    resolved_by   bigint
);
CREATE INDEX event_dead_letter_state_idx ON kernel.event_dead_letter (state, created_at);

CREATE TABLE kernel.event_replay (
    id            uuid PRIMARY KEY,
    subscription  text NOT NULL,
    event_types   text[] NOT NULL,
    from_ts       timestamptz NOT NULL,
    to_ts         timestamptz NOT NULL,
    force         boolean NOT NULL DEFAULT false,
    rate_per_sec  integer NOT NULL DEFAULT 2000,
    state         text NOT NULL DEFAULT 'pending' CHECK (state IN ('pending','running','done','failed','cancelled')),
    progress      jsonb NOT NULL DEFAULT '{}',
    created_by    bigint,
    created_at    timestamptz NOT NULL DEFAULT now(),
    finished_at   timestamptz
);

CREATE TABLE kernel.cron_run (
    run_id        uuid PRIMARY KEY,
    plugin_id     text NOT NULL,
    job           text NOT NULL,
    kind          text NOT NULL CHECK (kind IN ('cron','manual')),
    scheduled_at  timestamptz NOT NULL,
    started_at    timestamptz NOT NULL,
    finished_at   timestamptz,
    node_id       text NOT NULL,
    outcome       text NOT NULL DEFAULT 'running' CHECK (outcome IN ('running','ok','failed','timeout','skipped','lock_lost')),
    error         jsonb
);
CREATE INDEX cron_run_job_idx ON kernel.cron_run (plugin_id, job, scheduled_at DESC);

CREATE TABLE kernel.delayed_job (
    id          uuid PRIMARY KEY,
    plugin_id   text NOT NULL,
    name        text NOT NULL,
    run_at      timestamptz NOT NULL,
    payload     jsonb NOT NULL,
    dedupe_key  text,
    state       text NOT NULL DEFAULT 'pending' CHECK (state IN ('pending','running','done','failed','cancelled')),
    attempts    integer NOT NULL DEFAULT 0,
    last_error  jsonb,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX delayed_job_run_at_idx ON kernel.delayed_job (run_at) WHERE state = 'pending';
CREATE UNIQUE INDEX delayed_job_dedupe_key ON kernel.delayed_job (plugin_id, name, dedupe_key)
    WHERE dedupe_key IS NOT NULL AND state IN ('pending','running');

CREATE TABLE kernel.blob_usage (
    plugin_id  text PRIMARY KEY,
    bytes      bigint NOT NULL DEFAULT 0,
    objects    bigint NOT NULL DEFAULT 0
);
