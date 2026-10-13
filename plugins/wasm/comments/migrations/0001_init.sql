-- comments（LLD §3）
CREATE TABLE target_counters (
    target         text PRIMARY KEY,
    next_floor     integer NOT NULL DEFAULT 1,
    comment_count  integer NOT NULL DEFAULT 0,
    hidden         boolean NOT NULL DEFAULT false
);

CREATE TABLE comments (
    id               bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    target           text NOT NULL,
    floor            integer NOT NULL,
    author_id        bigint,
    reply_to         bigint REFERENCES comments (id),
    markup           text NOT NULL CHECK (markup IN ('markdown', 'bbcode')),
    source           text NOT NULL,
    rendered         text NOT NULL,
    plain            text NOT NULL,
    render_version   integer NOT NULL,
    state            text NOT NULL DEFAULT 'visible' CHECK (state IN ('visible', 'deleted_by_author', 'deleted_by_moderator', 'purged')),
    deleted_reason   text,
    thanks_count     integer NOT NULL DEFAULT 0,
    idempotency_key  text,
    created_at       timestamptz NOT NULL DEFAULT now(),
    edited_at        timestamptz,
    UNIQUE (target, floor)
);
CREATE INDEX comments_reply_idx ON comments (reply_to) WHERE reply_to IS NOT NULL;
CREATE INDEX comments_author_idx ON comments (author_id, id DESC);
CREATE UNIQUE INDEX comments_idem_idx ON comments (author_id, idempotency_key) WHERE idempotency_key IS NOT NULL;

CREATE TABLE edits (
    id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    comment_id  bigint NOT NULL REFERENCES comments (id),
    editor_id   bigint NOT NULL,
    old_source  text NOT NULL,
    edited_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX edits_comment_idx ON edits (comment_id, id DESC);

CREATE TABLE thanks (
    comment_id  bigint NOT NULL REFERENCES comments (id),
    user_id     bigint NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (comment_id, user_id)
);
