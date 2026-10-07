-- accounting 0001（docs/plugins/accounting/03 §3.1 的 P2 子集）。search_path = accounting。
-- 未包含：signals、swarm_stats、snapshot、torrent_user_hourly、recalc、reconcile（后续迭代）。

CREATE TABLE stream_cursors (
    part          smallint PRIMARY KEY CHECK (part >= 0),
    last_id       text NOT NULL DEFAULT '0-0' CHECK (last_id ~ '^\d+-\d+$'),
    last_ms       bigint NOT NULL DEFAULT 0,
    watermark_ms  bigint NOT NULL DEFAULT 0,
    idle_since    timestamptz,
    fence         bigint NOT NULL DEFAULT 0,
    entries_total bigint NOT NULL DEFAULT 0,
    committed_at  timestamptz,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE rejected_entries (
    part        smallint NOT NULL,
    stream_id   text NOT NULL,
    reason      text NOT NULL CHECK (char_length(reason) <= 64),
    fields      jsonb NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (part, stream_id)
);

CREATE TABLE torrents (
    id          bigint PRIMARY KEY,
    size        bigint NOT NULL DEFAULT 0 CHECK (size >= 0),
    status      text NOT NULL DEFAULT 'approved',
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE peers (
    user_id          bigint NOT NULL,
    torrent_id       bigint NOT NULL,
    peer_id          bytea NOT NULL CHECK (octet_length(peer_id) = 20),
    part             smallint NOT NULL,
    is_seeder        boolean NOT NULL,
    is_active        boolean NOT NULL DEFAULT true,
    client_prefix    text CHECK (char_length(client_prefix) <= 8),
    first_seen_at    timestamptz NOT NULL,
    last_announce_at timestamptz NOT NULL,
    PRIMARY KEY (user_id, torrent_id, peer_id)
) PARTITION BY HASH (user_id);
CREATE TABLE peers_p00 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 0) WITH (fillfactor = 70);
CREATE TABLE peers_p01 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 1) WITH (fillfactor = 70);
CREATE TABLE peers_p02 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 2) WITH (fillfactor = 70);
CREATE TABLE peers_p03 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 3) WITH (fillfactor = 70);
CREATE TABLE peers_p04 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 4) WITH (fillfactor = 70);
CREATE TABLE peers_p05 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 5) WITH (fillfactor = 70);
CREATE TABLE peers_p06 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 6) WITH (fillfactor = 70);
CREATE TABLE peers_p07 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 7) WITH (fillfactor = 70);
CREATE TABLE peers_p08 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 8) WITH (fillfactor = 70);
CREATE TABLE peers_p09 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 9) WITH (fillfactor = 70);
CREATE TABLE peers_p10 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 10) WITH (fillfactor = 70);
CREATE TABLE peers_p11 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 11) WITH (fillfactor = 70);
CREATE TABLE peers_p12 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 12) WITH (fillfactor = 70);
CREATE TABLE peers_p13 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 13) WITH (fillfactor = 70);
CREATE TABLE peers_p14 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 14) WITH (fillfactor = 70);
CREATE TABLE peers_p15 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 15) WITH (fillfactor = 70);
CREATE TABLE peers_p16 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 16) WITH (fillfactor = 70);
CREATE TABLE peers_p17 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 17) WITH (fillfactor = 70);
CREATE TABLE peers_p18 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 18) WITH (fillfactor = 70);
CREATE TABLE peers_p19 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 19) WITH (fillfactor = 70);
CREATE TABLE peers_p20 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 20) WITH (fillfactor = 70);
CREATE TABLE peers_p21 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 21) WITH (fillfactor = 70);
CREATE TABLE peers_p22 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 22) WITH (fillfactor = 70);
CREATE TABLE peers_p23 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 23) WITH (fillfactor = 70);
CREATE TABLE peers_p24 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 24) WITH (fillfactor = 70);
CREATE TABLE peers_p25 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 25) WITH (fillfactor = 70);
CREATE TABLE peers_p26 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 26) WITH (fillfactor = 70);
CREATE TABLE peers_p27 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 27) WITH (fillfactor = 70);
CREATE TABLE peers_p28 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 28) WITH (fillfactor = 70);
CREATE TABLE peers_p29 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 29) WITH (fillfactor = 70);
CREATE TABLE peers_p30 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 30) WITH (fillfactor = 70);
CREATE TABLE peers_p31 PARTITION OF peers FOR VALUES WITH (MODULUS 32, REMAINDER 31) WITH (fillfactor = 70);
CREATE INDEX peers_torrent_id_idx ON peers (torrent_id);

CREATE TABLE snatches (
    user_id          bigint NOT NULL,
    torrent_id       bigint NOT NULL,
    uploaded_raw     bigint NOT NULL DEFAULT 0 CHECK (uploaded_raw >= 0),
    downloaded_raw   bigint NOT NULL DEFAULT 0 CHECK (downloaded_raw >= 0),
    uploaded         bigint NOT NULL DEFAULT 0 CHECK (uploaded >= 0),
    downloaded       bigint NOT NULL DEFAULT 0 CHECK (downloaded >= 0),
    seed_secs        bigint NOT NULL DEFAULT 0,
    leech_secs       bigint NOT NULL DEFAULT 0,
    first_action_at  timestamptz NOT NULL,
    last_action_at   timestamptz NOT NULL,
    completed_at     timestamptz,
    times_completed  integer NOT NULL DEFAULT 0,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, torrent_id)
) PARTITION BY HASH (user_id);
CREATE TABLE snatches_p00 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 0);
CREATE TABLE snatches_p01 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 1);
CREATE TABLE snatches_p02 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 2);
CREATE TABLE snatches_p03 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 3);
CREATE TABLE snatches_p04 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 4);
CREATE TABLE snatches_p05 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 5);
CREATE TABLE snatches_p06 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 6);
CREATE TABLE snatches_p07 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 7);
CREATE TABLE snatches_p08 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 8);
CREATE TABLE snatches_p09 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 9);
CREATE TABLE snatches_p10 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 10);
CREATE TABLE snatches_p11 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 11);
CREATE TABLE snatches_p12 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 12);
CREATE TABLE snatches_p13 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 13);
CREATE TABLE snatches_p14 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 14);
CREATE TABLE snatches_p15 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 15);
CREATE TABLE snatches_p16 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 16);
CREATE TABLE snatches_p17 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 17);
CREATE TABLE snatches_p18 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 18);
CREATE TABLE snatches_p19 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 19);
CREATE TABLE snatches_p20 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 20);
CREATE TABLE snatches_p21 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 21);
CREATE TABLE snatches_p22 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 22);
CREATE TABLE snatches_p23 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 23);
CREATE TABLE snatches_p24 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 24);
CREATE TABLE snatches_p25 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 25);
CREATE TABLE snatches_p26 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 26);
CREATE TABLE snatches_p27 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 27);
CREATE TABLE snatches_p28 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 28);
CREATE TABLE snatches_p29 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 29);
CREATE TABLE snatches_p30 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 30);
CREATE TABLE snatches_p31 PARTITION OF snatches FOR VALUES WITH (MODULUS 32, REMAINDER 31);
CREATE INDEX snatches_torrent_id_idx ON snatches (torrent_id, completed_at DESC);

CREATE TABLE user_totals (
    user_id          bigint PRIMARY KEY,
    uploaded_raw     bigint NOT NULL DEFAULT 0,
    downloaded_raw   bigint NOT NULL DEFAULT 0,
    uploaded_traffic bigint NOT NULL DEFAULT 0,
    downloaded_traffic bigint NOT NULL DEFAULT 0,
    uploaded_adjust  bigint NOT NULL DEFAULT 0,
    downloaded_adjust bigint NOT NULL DEFAULT 0,
    seed_secs        bigint NOT NULL DEFAULT 0,
    leech_secs       bigint NOT NULL DEFAULT 0,
    completed_count  integer NOT NULL DEFAULT 0,
    seeding_completed_count integer NOT NULL DEFAULT 0,
    seeding_count    integer NOT NULL DEFAULT 0,
    seeding_size     bigint NOT NULL DEFAULT 0,
    hidden           boolean NOT NULL DEFAULT false,
    purged_at        timestamptz,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT user_totals_uploaded_nonneg_check CHECK (uploaded_traffic + uploaded_adjust >= 0),
    CONSTRAINT user_totals_downloaded_nonneg_check CHECK (downloaded_traffic + downloaded_adjust >= 0)
);

CREATE TABLE user_daily (
    day          date NOT NULL,
    user_id      bigint NOT NULL,
    uploaded_raw bigint NOT NULL DEFAULT 0, downloaded_raw bigint NOT NULL DEFAULT 0,
    uploaded     bigint NOT NULL DEFAULT 0, downloaded bigint NOT NULL DEFAULT 0,
    seed_secs    bigint NOT NULL DEFAULT 0, leech_secs bigint NOT NULL DEFAULT 0,
    PRIMARY KEY (day, user_id)
) PARTITION BY RANGE (day);

CREATE TABLE settle_buffer (
    window_start  bigint NOT NULL,
    part          smallint NOT NULL,
    user_id       bigint NOT NULL,
    torrent_id    bigint NOT NULL,
    rate_key      bigint NOT NULL,
    up_permille   integer NOT NULL, down_permille integer NOT NULL,
    rule_ids      bigint[] NOT NULL,
    up_raw bigint NOT NULL DEFAULT 0, down_raw bigint NOT NULL DEFAULT 0,
    up bigint NOT NULL DEFAULT 0, down bigint NOT NULL DEFAULT 0,
    seed_secs bigint NOT NULL DEFAULT 0, leech_secs bigint NOT NULL DEFAULT 0,
    completed boolean NOT NULL DEFAULT false,
    PRIMARY KEY (part, window_start, user_id, torrent_id, rate_key)
);

CREATE TABLE traffic_segments (
    day           date NOT NULL,
    user_id       bigint NOT NULL,
    torrent_id    bigint NOT NULL,
    rate_key      bigint NOT NULL,
    up_permille   integer NOT NULL, down_permille integer NOT NULL,
    rule_ids      bigint[] NOT NULL,
    up_raw bigint NOT NULL, down_raw bigint NOT NULL, up bigint NOT NULL, down bigint NOT NULL,
    PRIMARY KEY (day, user_id, torrent_id, rate_key)
) PARTITION BY RANGE (day);

CREATE TABLE torrent_counters (
    torrent_id bigint PRIMARY KEY,
    completed  integer NOT NULL DEFAULT 0,
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE rate_rules (
    id             bigint GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
    owner_plugin   text NOT NULL CHECK (char_length(owner_plugin) <= 32),
    external_ref   text NOT NULL CHECK (external_ref ~ '^[A-Za-z0-9:._-]{1,128}$'),
    scope_kind     text NOT NULL CHECK (scope_kind IN ('global','torrents','users','user_torrent')),
    up_permille    integer CHECK (up_permille BETWEEN 0 AND 100000),
    down_permille  integer CHECK (down_permille BETWEEN 0 AND 10000),
    mode           text NOT NULL CHECK (mode IN ('favorable','override')),
    priority       integer NOT NULL DEFAULT 0,
    starts_at      timestamptz NOT NULL,
    ends_at        timestamptz,
    removed_at     timestamptz,
    supersedes     bigint REFERENCES rate_rules (id),
    reason_term    text CHECK (char_length(reason_term) <= 128),
    content_hash   bytea NOT NULL,
    is_current     boolean NOT NULL DEFAULT true,
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT rate_rules_has_rate_check CHECK (up_permille IS NOT NULL OR down_permille IS NOT NULL),
    CONSTRAINT rate_rules_time_check CHECK (ends_at IS NULL OR ends_at > starts_at)
);
CREATE UNIQUE INDEX rate_rules_current_key ON rate_rules (owner_plugin, external_ref) WHERE is_current;

CREATE TABLE rate_rule_targets (
    rule_id    bigint NOT NULL REFERENCES rate_rules (id),
    torrent_id bigint NOT NULL DEFAULT 0,
    user_id    bigint NOT NULL DEFAULT 0,
    PRIMARY KEY (rule_id, torrent_id, user_id)
);

CREATE TABLE adjustments (
    id               bigint GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
    user_id          bigint NOT NULL,
    d_up             bigint NOT NULL,
    d_down           bigint NOT NULL,
    applied_up       bigint NOT NULL,
    applied_down     bigint NOT NULL,
    reason           text NOT NULL CHECK (reason ~ '^[a-z][a-z0-9-]*\.[a-z][a-z0-9_]*$'),
    note             text CHECK (char_length(note) <= 500),
    caller_plugin    text NOT NULL,
    actor_user_id    bigint,
    idempotency_key  text NOT NULL CHECK (char_length(idempotency_key) <= 128),
    request_hash     bytea NOT NULL,
    created_at       timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX adjustments_idem_key ON adjustments (caller_plugin, idempotency_key);

-- ---------- 对外视图（评审 R1 F-03） ----------
CREATE VIEW v_user_totals WITH (security_barrier = true) AS
SELECT user_id,
       uploaded_traffic + uploaded_adjust AS uploaded,
       downloaded_traffic + downloaded_adjust AS downloaded,
       uploaded_raw, downloaded_raw,
       seed_secs, leech_secs, completed_count, seeding_completed_count, seeding_count, seeding_size, updated_at
FROM user_totals WHERE NOT hidden AND purged_at IS NULL;

CREATE VIEW v_snatch WITH (security_barrier = true) AS
SELECT user_id, torrent_id, uploaded, downloaded, seed_secs, leech_secs, completed_at, last_action_at
FROM snatches;

CREATE VIEW v_user_daily WITH (security_barrier = true) AS
SELECT user_id, day, uploaded, downloaded, uploaded_raw, downloaded_raw, seed_secs, leech_secs FROM user_daily;
