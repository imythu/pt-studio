-- checkin（LLD §3 的签到部分）
CREATE TABLE user_state (
    user_id     bigint PRIMARY KEY,
    streak      integer NOT NULL DEFAULT 0,
    longest     integer NOT NULL DEFAULT 0,
    total_days  integer NOT NULL DEFAULT 0,
    last_day    date,
    updated_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE records (
    id                bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id           bigint NOT NULL,
    day               date NOT NULL,
    streak_after      integer NOT NULL,
    reward_daily      bigint NOT NULL DEFAULT 0,
    reward_milestone  bigint NOT NULL DEFAULT 0,
    milestone_days    integer,
    currency          text NOT NULL,
    payout_status     text NOT NULL DEFAULT 'pending' CHECK (payout_status IN ('pending', 'paid', 'none', 'failed')),
    via               text NOT NULL,
    created_at        timestamptz NOT NULL DEFAULT now(),
    UNIQUE (user_id, day)
);
CREATE INDEX records_day_idx ON records (day);
