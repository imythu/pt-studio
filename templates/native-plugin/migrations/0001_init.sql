CREATE TABLE welcomed (
    user_id     bigint PRIMARY KEY,
    welcomed_at timestamptz NOT NULL DEFAULT now()
);
