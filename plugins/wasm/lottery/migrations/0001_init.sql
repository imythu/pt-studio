-- 期次：seed_commit = sha256(seed)，开售前公开；seed 开奖后公开；结果由 seed 与 tickets_root（最后一个票据批次的链哈希）派生
CREATE TABLE rounds (
    id            bigserial PRIMARY KEY,
    title         text NOT NULL CHECK (char_length(title) BETWEEN 1 AND 100),
    description   text NOT NULL DEFAULT '' CHECK (char_length(description) <= 1000),
    ticket_price  bigint NOT NULL CHECK (ticket_price > 0),
    max_per_user  integer NOT NULL DEFAULT 0 CHECK (max_per_user >= 0),
    min_tickets   integer NOT NULL DEFAULT 1 CHECK (min_tickets >= 1),
    house_cut_pct integer NOT NULL DEFAULT 10 CHECK (house_cut_pct BETWEEN 0 AND 50),
    tiers         jsonb NOT NULL,
    opens_at      timestamptz NOT NULL DEFAULT now(),
    closes_at     timestamptz NOT NULL,
    state         text NOT NULL DEFAULT 'open' CHECK (state IN ('open', 'drawn', 'cancelled')),
    seed          bytea NOT NULL,
    seed_commit   bytea NOT NULL,
    tickets_root  bytea NOT NULL DEFAULT '\x'::bytea,
    total_tickets integer NOT NULL DEFAULT 0,
    pool          bigint NOT NULL DEFAULT 0,
    cancel_reason text,
    created_by    bigint,
    created_at    timestamptz NOT NULL DEFAULT now(),
    finished_at   timestamptz,
    CHECK (closes_at > opens_at)
);
CREATE INDEX rounds_open_idx ON rounds (closes_at) WHERE state = 'open';

-- 票据批次：每次购买一行，票号 first_no .. first_no + count - 1；chain = sha256(上一批 chain ‖ 批次字段)
CREATE TABLE tickets (
    id         bigserial PRIMARY KEY,
    round_id   bigint NOT NULL REFERENCES rounds (id),
    user_id    bigint NOT NULL,
    first_no   integer NOT NULL,
    count      integer NOT NULL CHECK (count > 0),
    cost       bigint NOT NULL,
    chain      bytea NOT NULL,
    refunded   boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (round_id, first_no)
);
CREATE INDEX tickets_user_idx ON tickets (user_id, id DESC);

CREATE TABLE prizes (
    id        bigserial PRIMARY KEY,
    round_id  bigint NOT NULL REFERENCES rounds (id),
    tier      integer NOT NULL,
    ticket_no integer NOT NULL,
    user_id   bigint NOT NULL,
    amount    bigint NOT NULL,
    paid      boolean NOT NULL DEFAULT false,
    UNIQUE (round_id, ticket_no)
);
CREATE INDEX prizes_user_idx ON prizes (user_id, id DESC);
