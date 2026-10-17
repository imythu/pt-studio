-- 上传的图片：内容寻址存储（blob `public/i/<前两位>/<sha256>.<ext>`），同一用户重复上传返回同一行
CREATE TABLE images (
    id         bigserial PRIMARY KEY,
    user_id    bigint NOT NULL,
    sha256     bytea NOT NULL,
    blob_key   text NOT NULL,
    mime       text NOT NULL,
    size       integer NOT NULL,
    width      integer NOT NULL,
    height     integer NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz,
    deleted_by bigint
);
CREATE UNIQUE INDEX images_user_sha_idx ON images (user_id, sha256) WHERE deleted_at IS NULL;
CREATE INDEX images_user_time_idx ON images (user_id, created_at DESC) WHERE deleted_at IS NULL;
CREATE INDEX images_key_idx ON images (blob_key) WHERE deleted_at IS NULL;
CREATE INDEX images_time_idx ON images (created_at DESC);
