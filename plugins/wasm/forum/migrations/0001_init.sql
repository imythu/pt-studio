-- forum（docs/plugins/forum/03 §3 的子集）：分区、版块、主题、楼层、投票、已读、订阅、感谢、编辑历史。
CREATE TABLE categories (
    id     bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name   text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 60),
    sort   integer NOT NULL DEFAULT 100
);

CREATE TABLE boards (
    id             bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    category_id    bigint NOT NULL REFERENCES categories (id),
    name           text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 60),
    description    text CHECK (char_length(description) <= 300),
    sort           integer NOT NULL DEFAULT 100,
    -- {"read": {"min_class": null, "roles": []}, "reply": {...}, "create": {...}}
    acl            jsonb NOT NULL DEFAULT '{}',
    topic_count    integer NOT NULL DEFAULT 0,
    post_count     integer NOT NULL DEFAULT 0,
    last_topic_id  bigint,
    last_post_at   timestamptz,
    active         boolean NOT NULL DEFAULT true
);

CREATE TABLE board_moderators (
    board_id  bigint NOT NULL REFERENCES boards (id) ON DELETE CASCADE,
    user_id   bigint NOT NULL,
    PRIMARY KEY (board_id, user_id)
);

CREATE TABLE topics (
    id              bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    board_id        bigint NOT NULL REFERENCES boards (id),
    title           text NOT NULL CHECK (char_length(title) BETWEEN 1 AND 120),
    author_id       bigint,
    last_post_id    bigint,
    last_post_at    timestamptz NOT NULL DEFAULT now(),
    last_poster_id  bigint,
    post_count      integer NOT NULL DEFAULT 0,
    next_floor      integer NOT NULL DEFAULT 1,
    sticky          boolean NOT NULL DEFAULT false,
    announcement    boolean NOT NULL DEFAULT false,
    featured        boolean NOT NULL DEFAULT false,
    locked          boolean NOT NULL DEFAULT false,
    state           text NOT NULL DEFAULT 'visible' CHECK (state IN ('visible', 'deleted')),
    idempotency_key text,
    created_at      timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX topics_board_idx ON topics (board_id, sticky DESC, last_post_at DESC) WHERE state = 'visible';
CREATE INDEX topics_announce_idx ON topics (last_post_at DESC) WHERE announcement AND state = 'visible';
CREATE UNIQUE INDEX topics_idem_key ON topics (author_id, idempotency_key) WHERE idempotency_key IS NOT NULL;

CREATE TABLE posts (
    id              bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    topic_id        bigint NOT NULL REFERENCES topics (id),
    floor           integer NOT NULL,
    author_id       bigint,
    markup          text NOT NULL,
    source          text NOT NULL,
    rendered        text NOT NULL,
    plain           text NOT NULL,
    render_version  integer NOT NULL,
    state           text NOT NULL DEFAULT 'visible' CHECK (state IN ('visible', 'deleted_by_author', 'deleted_by_moderator')),
    thanks_count    integer NOT NULL DEFAULT 0,
    idempotency_key text,
    created_at      timestamptz NOT NULL DEFAULT now(),
    edited_at       timestamptz,
    UNIQUE (topic_id, floor)
);
CREATE INDEX posts_author_idx ON posts (author_id, id DESC);
CREATE UNIQUE INDEX posts_idem_key ON posts (author_id, idempotency_key) WHERE idempotency_key IS NOT NULL;

CREATE TABLE post_edits (
    id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    post_id     bigint NOT NULL REFERENCES posts (id),
    editor_id   bigint NOT NULL,
    old_source  text NOT NULL,
    edited_at   timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE polls (
    topic_id       bigint PRIMARY KEY REFERENCES topics (id),
    question       text NOT NULL CHECK (char_length(question) BETWEEN 1 AND 200),
    options        text[] NOT NULL CHECK (array_length(options, 1) BETWEEN 2 AND 20),
    counts         integer[] NOT NULL,
    max_choices    integer NOT NULL DEFAULT 1,
    closes_at      timestamptz
);
CREATE TABLE poll_votes (
    topic_id    bigint NOT NULL,
    user_id     bigint NOT NULL,
    option_idx  smallint NOT NULL,
    PRIMARY KEY (topic_id, user_id, option_idx)
);

CREATE TABLE topic_reads (
    user_id          bigint NOT NULL,
    topic_id         bigint NOT NULL,
    last_read_floor  integer NOT NULL,
    updated_at       timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, topic_id)
);
-- “全部标为已读”：之前的主题视为已读
CREATE TABLE user_catchup (
    user_id  bigint PRIMARY KEY,
    at       timestamptz NOT NULL
);

CREATE TABLE subscriptions (
    user_id   bigint NOT NULL,
    topic_id  bigint NOT NULL,
    PRIMARY KEY (user_id, topic_id)
);
CREATE INDEX subscriptions_topic_idx ON subscriptions (topic_id);
CREATE TABLE subs_dirty (
    topic_id  bigint PRIMARY KEY,
    since     timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE thanks (
    post_id  bigint NOT NULL,
    user_id  bigint NOT NULL,
    PRIMARY KEY (post_id, user_id)
);

-- 其他对象（如种子）的讨论帖
CREATE TABLE topic_links (
    target    text PRIMARY KEY CHECK (char_length(target) <= 80),
    topic_id  bigint NOT NULL REFERENCES topics (id)
);
