-- 商品：类型决定履约方式，params 为类型参数（如上传量字节数、邀请名额数）
CREATE TABLE items (
    id             bigserial PRIMARY KEY,
    kind           text NOT NULL CHECK (kind IN ('upload_credit', 'invite', 'promo_card', 'hnr_removal')),
    title          text NOT NULL CHECK (char_length(title) BETWEEN 1 AND 100),
    description    text NOT NULL DEFAULT '' CHECK (char_length(description) <= 500),
    price          bigint NOT NULL CHECK (price > 0),
    params         jsonb NOT NULL DEFAULT '{}',
    stock          integer CHECK (stock >= 0),
    per_user_limit integer NOT NULL DEFAULT 0 CHECK (per_user_limit >= 0),
    limit_period   text NOT NULL DEFAULT 'none' CHECK (limit_period IN ('none', 'day', 'week', 'month')),
    active         boolean NOT NULL DEFAULT false,
    sort           integer NOT NULL DEFAULT 0,
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz NOT NULL DEFAULT now()
);

-- 订单：id 同时用于派生冻结 / 履约 / 扣款的幂等键
CREATE TABLE orders (
    id         bigserial PRIMARY KEY,
    user_id    bigint NOT NULL,
    item_id    bigint NOT NULL REFERENCES items (id),
    kind       text NOT NULL,
    item_title text NOT NULL,
    quantity   integer NOT NULL CHECK (quantity BETWEEN 1 AND 100),
    total      bigint NOT NULL,
    target     jsonb NOT NULL DEFAULT '{}',
    state      text NOT NULL DEFAULT 'completed' CHECK (state IN ('completed', 'refunded')),
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX orders_user_idx ON orders (user_id, created_at DESC);
CREATE INDEX orders_item_user_idx ON orders (item_id, user_id, created_at);
CREATE INDEX orders_time_idx ON orders (created_at DESC);

-- 预置商品（默认下架，管理员确认价格后上架）
INSERT INTO items (kind, title, description, price, params, per_user_limit, limit_period, sort) VALUES
  ('upload_credit', '10 GiB 上传量', '立即增加 10 GiB 上传量', 2000, '{"bytes": 10737418240}', 0, 'none', 10),
  ('invite', '邀请名额 × 1', '30 天内有效', 8000, '{"count": 1, "expires_days": 30}', 1, 'month', 20),
  ('promo_card', '免费卡 × 1', '促销卡：可对种子使用的免费卡', 3000, '{"card_kind": "free", "count": 1}', 5, 'month', 30),
  ('hnr_removal', '消除一条 H&R', '消除一条进行中或已失败的 H&R 记录', 5000, '{}', 3, 'month', 40);
