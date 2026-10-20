-- 内置头像换成动漫插画（.jpg）：仍在用旧矢量头像（/avatars/*.svg，文件已删除）的用户随机换一个新的
UPDATE users SET avatar_url = (ARRAY['/avatars/frieren.jpg', '/avatars/frieren-reading.jpg', '/avatars/frieren-sky.jpg', '/avatars/frieren-cool.jpg', '/avatars/frieren-sofa.jpg', '/avatars/fern-sofa.jpg', '/avatars/blonde-elf.jpg', '/avatars/gaze.jpg', '/avatars/staff.jpg'])[1 + floor(random() * 9)::int]
 WHERE avatar_url LIKE '/avatars/%.svg';
