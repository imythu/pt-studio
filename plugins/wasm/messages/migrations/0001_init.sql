-- messages（LLD §3）
CREATE TABLE conversations (
    id                 bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    kind               text NOT NULL CHECK (kind IN ('user', 'system', 'staff')),
    subject            text NOT NULL,
    created_by         bigint,
    created_by_plugin  text,
    last_message_id    bigint,
    last_message_at    timestamptz NOT NULL DEFAULT now(),
    created_at         timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE participants (
    conversation_id       bigint NOT NULL REFERENCES conversations (id) ON DELETE CASCADE,
    user_id               bigint NOT NULL,
    in_inbox              boolean NOT NULL DEFAULT false,
    in_sent               boolean NOT NULL DEFAULT false,
    folder                text NOT NULL DEFAULT 'normal' CHECK (folder IN ('normal', 'system')),
    last_read_message_id  bigint NOT NULL DEFAULT 0,
    unread_count          integer NOT NULL DEFAULT 0,
    removed_at            timestamptz,
    PRIMARY KEY (conversation_id, user_id)
);
CREATE INDEX participants_user_idx ON participants (user_id, conversation_id DESC) WHERE removed_at IS NULL;

CREATE TABLE messages (
    id               bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    conversation_id  bigint NOT NULL REFERENCES conversations (id) ON DELETE CASCADE,
    sender_id        bigint,
    markup           text NOT NULL,
    source           text NOT NULL,
    rendered         text NOT NULL,
    plain            text NOT NULL,
    render_version   integer NOT NULL,
    created_at       timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX messages_conv_idx ON messages (conversation_id, id);

CREATE TABLE blocks (
    user_id     bigint NOT NULL,
    blocked_id  bigint NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, blocked_id)
);

CREATE TABLE settings (
    user_id  bigint PRIMARY KEY,
    accept   text NOT NULL CHECK (accept IN ('all', 'staff_only'))
);
