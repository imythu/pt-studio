-- 内置头像：给还没有头像的已有用户随机分配一个（新注册用户在创建时分配，见 accounts.rs）
UPDATE users SET avatar_url = (ARRAY['/avatars/frieren.svg', '/avatars/fern.svg', '/avatars/stark.svg', '/avatars/himmel.svg', '/avatars/heiter.svg', '/avatars/eisen.svg', '/avatars/sein.svg', '/avatars/ubel.svg', '/avatars/land.svg', '/avatars/denken.svg', '/avatars/serie.svg', '/avatars/flamme.svg', '/avatars/methode.svg', '/avatars/lawine.svg'])[1 + floor(random() * 14)::int]
 WHERE avatar_url IS NULL;
