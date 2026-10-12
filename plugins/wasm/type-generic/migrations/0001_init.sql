-- type-generic（LLD §3）
CREATE TABLE kinds (
    kind        text PRIMARY KEY CHECK (kind ~ '^[a-z][a-z0-9_]{0,31}$'),
    definition  jsonb NOT NULL,
    compiled    jsonb NOT NULL,
    version     integer NOT NULL DEFAULT 1,
    active      boolean NOT NULL DEFAULT true,
    preset      boolean NOT NULL DEFAULT false,
    updated_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE vocab (
    kind   text NOT NULL,
    field  text NOT NULL,
    alias  text NOT NULL,
    value  text NOT NULL,
    PRIMARY KEY (kind, field, alias)
);
