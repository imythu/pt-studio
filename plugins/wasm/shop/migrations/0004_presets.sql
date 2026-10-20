-- 预置商品直接上架（此前默认下架，商城首次打开是空的）；管理员改动过的商品不受影响。另补一个 VIP 商品。
UPDATE items SET active = true, updated_at = now()
 WHERE NOT active AND updated_at = created_at
   AND kind IN ('upload_credit', 'invite', 'promo_card', 'hnr_removal');
INSERT INTO items (kind, title, description, price, params, per_user_limit, limit_period, sort, active)
SELECT 'vip', 'VIP × 7 天', '7 天 VIP（已有 VIP 时顺延）', 20000, '{"days": 7}', 2, 'month', 50, true
 WHERE NOT EXISTS (SELECT 1 FROM items WHERE kind = 'vip');
