-- 收藏（私有）
CREATE TABLE bookmarks (
    user_id    bigint NOT NULL,
    torrent_id bigint NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, torrent_id)
);
CREATE INDEX bookmarks_torrent_idx ON bookmarks (torrent_id);
CREATE INDEX bookmarks_user_time_idx ON bookmarks (user_id, created_at DESC);

-- 合集：用户整理的种子列表，可公开
CREATE TABLE collections (
    id          bigserial PRIMARY KEY,
    owner_id    bigint NOT NULL,
    title       text NOT NULL CHECK (char_length(title) BETWEEN 1 AND 100),
    description text NOT NULL DEFAULT '' CHECK (char_length(description) <= 5000),
    is_public   boolean NOT NULL DEFAULT true,
    item_count  integer NOT NULL DEFAULT 0,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX collections_owner_idx ON collections (owner_id, updated_at DESC);
CREATE INDEX collections_public_idx ON collections (updated_at DESC) WHERE is_public AND item_count > 0;

CREATE TABLE collection_items (
    collection_id bigint NOT NULL REFERENCES collections (id) ON DELETE CASCADE,
    torrent_id    bigint NOT NULL,
    position      integer NOT NULL,
    note          text NOT NULL DEFAULT '' CHECK (char_length(note) <= 500),
    added_at      timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (collection_id, torrent_id)
);
CREATE INDEX collection_items_torrent_idx ON collection_items (torrent_id);
