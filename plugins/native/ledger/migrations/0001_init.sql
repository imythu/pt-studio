-- ledger 0001（docs/plugins/ledger/03 §3.1）。search_path = ledger。
CREATE TABLE currencies (
    code          text PRIMARY KEY CHECK (code ~ '^[a-z][a-z0-9_]{1,15}$'),
    scale         smallint NOT NULL CHECK (scale BETWEEN 0 AND 6),
    name_term     text NOT NULL CHECK (char_length(name_term) <= 128),
    owner_plugin  text NOT NULL CHECK (char_length(owner_plugin) <= 32),
    created_at    timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE accounts (
    user_id     bigint NOT NULL,
    currency    text NOT NULL REFERENCES currencies (code),
    balance     bigint NOT NULL DEFAULT 0,
    held        bigint NOT NULL DEFAULT 0 CHECK (held >= 0),
    lock_count  integer NOT NULL DEFAULT 0 CHECK (lock_count >= 0),
    is_hidden   boolean NOT NULL DEFAULT false,
    version     integer NOT NULL DEFAULT 1,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, currency),
    CONSTRAINT accounts_balance_range_check CHECK (balance BETWEEN -9007199254740991 AND 9007199254740991)
);
CREATE INDEX accounts_currency_balance_idx ON accounts (currency, balance DESC);   -- 排行榜、对账

CREATE TABLE entries (
    id                  uuid NOT NULL,
    created_at          timestamptz NOT NULL,
    currency            text NOT NULL,
    kind                text NOT NULL CHECK (kind IN ('issue','burn','transfer','hold_capture','reversal','adjust')),
    reason              text NOT NULL CHECK (reason ~ '^[a-z][a-z0-9-]*\.[a-z][a-z0-9_]*$'),
    memo                text CHECK (char_length(memo) <= 200),
    caller_plugin       text NOT NULL,
    actor_kind          text NOT NULL CHECK (actor_kind IN ('anonymous','user','system','plugin','mcp')),
    actor_user_id       bigint,
    reversal_of         uuid,
    hold_id             uuid,
    PRIMARY KEY (created_at, id)
) PARTITION BY RANGE (created_at);
CREATE INDEX entries_id_idx ON entries (id);
CREATE INDEX entries_reversal_of_idx ON entries (reversal_of) WHERE reversal_of IS NOT NULL;

CREATE TABLE lines (
    entry_id              uuid NOT NULL,
    created_at            timestamptz NOT NULL,
    line_no               smallint NOT NULL,
    currency              text NOT NULL,
    user_id               bigint,                 -- NULL = 系统
    amount                bigint NOT NULL CHECK (amount <> 0),
    balance_after         bigint,                 -- 系统分录为 NULL
    counterparty_user_id  bigint,
    PRIMARY KEY (created_at, entry_id, line_no)
) PARTITION BY RANGE (created_at);
CREATE INDEX lines_user_currency_idx ON lines (user_id, currency, created_at DESC, entry_id DESC, line_no DESC)
    WHERE user_id IS NOT NULL;
CREATE INDEX lines_entry_id_idx ON lines (entry_id);

CREATE TABLE entry_reversals (                      -- 防重复冲正（跨分区唯一）
    entry_id            uuid PRIMARY KEY,
    reversal_entry_id   uuid,
    created_at          timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE idempotency (
    caller_plugin  text NOT NULL,
    key            text NOT NULL CHECK (char_length(key) BETWEEN 1 AND 128),
    request_hash   bytea NOT NULL,
    entry_id       uuid,
    result         jsonb,
    created_at     timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (caller_plugin, key)
);
CREATE INDEX idempotency_created_at_idx ON idempotency (created_at);

CREATE TABLE holds (
    id               uuid PRIMARY KEY,
    currency         text NOT NULL REFERENCES currencies (code),
    user_id          bigint NOT NULL,
    amount           bigint NOT NULL CHECK (amount > 0),
    remaining        bigint NOT NULL CHECK (remaining >= 0),
    captured_amount  bigint NOT NULL DEFAULT 0,
    state            text NOT NULL CHECK (state IN ('active','released','captured','expired')),
    reason           text NOT NULL,
    caller_plugin    text NOT NULL,
    expires_at       timestamptz NOT NULL,
    created_at       timestamptz NOT NULL DEFAULT now(),
    closed_at        timestamptz
);
CREATE INDEX holds_active_expires_idx ON holds (expires_at) WHERE state = 'active';
CREATE INDEX holds_user_idx ON holds (user_id, currency) WHERE state = 'active';

CREATE TABLE account_locks (
    id             uuid PRIMARY KEY,
    user_id        bigint NOT NULL,
    currency       text,                          -- NULL = 全部币种
    reason         text NOT NULL CHECK (char_length(reason) <= 64),
    ref            text CHECK (char_length(ref) <= 128),
    note           text CHECK (char_length(note) <= 500),
    caller_plugin  text NOT NULL,
    created_by     bigint,
    created_at     timestamptz NOT NULL DEFAULT now(),
    released_at    timestamptz,
    released_by    bigint
);
CREATE INDEX account_locks_active_idx ON account_locks (user_id) WHERE released_at IS NULL;

CREATE TABLE supply_shards (
    currency  text NOT NULL REFERENCES currencies (code),
    shard     smallint NOT NULL CHECK (shard BETWEEN 0 AND 15),
    issued    bigint NOT NULL DEFAULT 0,
    burned    bigint NOT NULL DEFAULT 0,
    PRIMARY KEY (currency, shard)
);

CREATE TABLE daily_outflow (
    day       date NOT NULL,
    user_id   bigint NOT NULL,
    currency  text NOT NULL,
    amount    bigint NOT NULL DEFAULT 0,
    PRIMARY KEY (day, user_id, currency)
);

CREATE TABLE checkpoints (
    month        date NOT NULL,                  -- 月初日期
    user_id      bigint NOT NULL,
    currency     text NOT NULL,
    balance_end  bigint NOT NULL,
    line_count   integer NOT NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (month, user_id, currency)
) PARTITION BY RANGE (month);

CREATE TABLE reconcile_reports (
    id           bigint GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
    day          date NOT NULL,
    check_name   text NOT NULL CHECK (check_name IN ('balance','entry_balance','supply','held','lock_count')),
    subject      text NOT NULL,
    expected     jsonb NOT NULL,
    actual       jsonb NOT NULL,
    resolved_at  timestamptz,
    resolved_by  bigint,
    created_at   timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX reconcile_reports_dedupe_key ON reconcile_reports (day, check_name, subject);

CREATE TABLE archived_partitions (
    table_name  text NOT NULL,
    partition   text NOT NULL,
    blob_key    text NOT NULL,
    rows        bigint NOT NULL,
    sha256      bytea NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (table_name, partition)
);

CREATE VIEW v_balance WITH (security_barrier = true) AS
SELECT user_id, currency, balance, updated_at FROM accounts WHERE NOT is_hidden;

-- 实现说明：币种可变设置在内核设置接口完成前存放于此表（03 §1.1 设计为插件运行时设置）
CREATE TABLE currency_settings (
    currency         text PRIMARY KEY REFERENCES currencies (code),
    transferable     boolean NOT NULL DEFAULT false,
    fee_bps          integer NOT NULL DEFAULT 0 CHECK (fee_bps BETWEEN 0 AND 10000),
    min_transfer     bigint NOT NULL DEFAULT 1 CHECK (min_transfer >= 1),
    daily_out_limit  bigint NOT NULL DEFAULT 0 CHECK (daily_out_limit >= 0),
    min_balance      bigint NOT NULL DEFAULT 0,
    max_hold_secs    integer NOT NULL DEFAULT 604800 CHECK (max_hold_secs > 0),
    disabled         boolean NOT NULL DEFAULT false,
    updated_at       timestamptz NOT NULL DEFAULT now()
);
