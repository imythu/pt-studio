-- hnr（LLD §3）。records 按月分区（[[database.partitions]]，内核预建）。
CREATE TABLE records (
    id                  bigint GENERATED ALWAYS AS IDENTITY,
    user_id             bigint NOT NULL,
    torrent_id          bigint NOT NULL,
    completed_at        timestamptz NOT NULL,
    deadline            timestamptz NOT NULL,
    state               text NOT NULL CHECK (state IN ('active','satisfied','failed','forgiven','exempted')),
    exempt_rule         text,
    prewarned           boolean NOT NULL DEFAULT false,
    seed_secs_snapshot  bigint NOT NULL DEFAULT 0,
    ratio_snapshot      double precision,
    resolved_at         timestamptz,
    failed_at           timestamptz,
    forgiven_by         text,
    forgive_reason      text,
    PRIMARY KEY (completed_at, id),
    UNIQUE (user_id, torrent_id, completed_at)
) PARTITION BY RANGE (completed_at);
CREATE INDEX records_active_deadline_idx ON records (deadline) WHERE state = 'active';
CREATE INDEX records_user_idx ON records (user_id, completed_at DESC);
CREATE INDEX records_torrent_active_idx ON records (torrent_id) WHERE state = 'active';
CREATE INDEX records_failed_idx ON records (user_id, failed_at) WHERE state = 'failed';
CREATE INDEX records_id_idx ON records (id);

CREATE TABLE user_counts (
    user_id            bigint PRIMARY KEY,
    active             integer NOT NULL DEFAULT 0,
    failed_effective   integer NOT NULL DEFAULT 0,
    restricted         boolean NOT NULL DEFAULT false,
    restricted_seq     integer NOT NULL DEFAULT 0,
    restricted_skipped boolean NOT NULL DEFAULT false,
    updated_at         timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE idempotency (
    caller      text NOT NULL,
    key         text NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (caller, key)
);
