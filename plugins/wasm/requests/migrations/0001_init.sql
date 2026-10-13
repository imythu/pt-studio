-- requests（LLD §3）
CREATE TABLE requests (
    id                 bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id            bigint,
    type_id            text NOT NULL,
    title              text NOT NULL,
    description_src    text NOT NULL,
    description_html   text NOT NULL,
    requirements       jsonb NOT NULL DEFAULT '[]',
    currency           text NOT NULL,
    bounty_total       bigint NOT NULL DEFAULT 0,
    votes              integer NOT NULL DEFAULT 0,
    state              text NOT NULL CHECK (state IN ('open', 'fill_pending', 'filled', 'closing', 'closed')),
    close_reason       text,
    expires_at         timestamptz NOT NULL,
    filled_torrent_id  bigint,
    created_at         timestamptz NOT NULL DEFAULT now(),
    updated_at         timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX requests_open_idx ON requests (type_id, bounty_total DESC) WHERE state IN ('open', 'fill_pending');
CREATE INDEX requests_expire_idx ON requests (expires_at) WHERE state = 'open';

CREATE TABLE contributions (
    id            bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    request_id    bigint NOT NULL REFERENCES requests (id),
    user_id       bigint NOT NULL,
    amount        bigint NOT NULL CHECK (amount > 0),
    state         text NOT NULL CHECK (state IN ('pending', 'escrowed', 'failed', 'refunded', 'paid')),
    paid_fill_id  bigint,
    created_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX contributions_request_idx ON contributions (request_id, state);

CREATE TABLE fills (
    id               bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    request_id       bigint NOT NULL REFERENCES requests (id),
    torrent_id       bigint NOT NULL,
    filler_id        bigint NOT NULL,
    state            text NOT NULL CHECK (state IN ('pending', 'rejected', 'paid_out')),
    reason           text,
    auto_confirm_at  timestamptz NOT NULL,
    confirmed_at     timestamptz,
    confirmed_by     text,
    payout_amount    bigint,
    created_at       timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX fills_one_pending_idx ON fills (request_id) WHERE state = 'pending';
CREATE INDEX fills_auto_idx ON fills (auto_confirm_at) WHERE state = 'pending';

CREATE TABLE votes (request_id bigint NOT NULL, user_id bigint NOT NULL, PRIMARY KEY (request_id, user_id));

CREATE VIEW v_request_public WITH (security_barrier = true) AS
SELECT id, type_id, title, state AS status, user_id, created_at FROM requests;
