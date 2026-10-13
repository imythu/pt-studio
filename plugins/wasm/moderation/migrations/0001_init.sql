-- moderation（LLD §3）
CREATE TABLE queue (
    target         text PRIMARY KEY,
    kind           text NOT NULL,
    state          text NOT NULL CHECK (state IN ('pending', 'candidate', 'awaiting_uploader')),
    type_id        text,
    uploader_id    bigint,
    submitted_at   timestamptz NOT NULL,
    ai_verdict     text,
    ai_findings    integer,
    claimed_by     bigint,
    claim_until    timestamptz
);
CREATE INDEX queue_state_idx ON queue (state, submitted_at);

CREATE TABLE decisions (
    id            bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    target        text NOT NULL,
    decision      text NOT NULL,
    note          text NOT NULL DEFAULT '',
    moderator_id  bigint,
    actor         text NOT NULL,
    snapshot      jsonb NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX decisions_target_idx ON decisions (target, id DESC);
CREATE INDEX decisions_moderator_idx ON decisions (moderator_id, id DESC);

CREATE TABLE uploader_stats (user_id bigint PRIMARY KEY, approved integer NOT NULL DEFAULT 0, rejected integer NOT NULL DEFAULT 0);

CREATE TABLE candidate_votes (
    torrent_id  bigint NOT NULL,
    user_id     bigint NOT NULL,
    value       smallint NOT NULL CHECK (value IN (-1, 1)),
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (torrent_id, user_id)
);

CREATE TABLE words (
    id       bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    pattern  text NOT NULL,
    kind     text NOT NULL CHECK (kind IN ('exact', 'wildcard')),
    action   text NOT NULL CHECK (action IN ('block', 'mask', 'review')),
    scopes   text[] NOT NULL,
    active   boolean NOT NULL DEFAULT true
);

CREATE TABLE word_hits (
    id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    scope       text NOT NULL,
    author_id   bigint,
    excerpt     text NOT NULL,
    state       text NOT NULL DEFAULT 'open',
    created_at  timestamptz NOT NULL DEFAULT now()
);
