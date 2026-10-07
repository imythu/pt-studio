-- 0002：peers 的小时做种槽（两个槽轮换，03 §1.5 第 2 步）与小时快照进度（03 §1.9）。

ALTER TABLE peers
    ADD COLUMN slot0_hour integer,
    ADD COLUMN slot0_secs integer NOT NULL DEFAULT 0,
    ADD COLUMN slot1_hour integer,
    ADD COLUMN slot1_secs integer NOT NULL DEFAULT 0;

CREATE TABLE snapshot_runs (
    hour          integer NOT NULL,              -- H(t) = floor(t / 3600)
    part          smallint NOT NULL,
    state         text NOT NULL DEFAULT 'running' CHECK (state IN ('running', 'done')),
    last_user_id  bigint NOT NULL DEFAULT 0,
    users         integer NOT NULL DEFAULT 0,
    started_at    timestamptz NOT NULL DEFAULT now(),
    finished_at   timestamptz,
    PRIMARY KEY (hour, part)
);
