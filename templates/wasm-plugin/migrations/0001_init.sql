-- 迁移在插件自己的 schema 中执行（schema 名即插件 ID），按文件名顺序只执行一次；已发布的迁移不要修改，新增 0002_…
CREATE TABLE notes (
    id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id     bigint NOT NULL,
    title       text NOT NULL CHECK (char_length(title) BETWEEN 1 AND 100),
    body        text NOT NULL DEFAULT '' CHECK (char_length(body) <= 10000),
    created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX notes_user_idx ON notes (user_id, id DESC);
