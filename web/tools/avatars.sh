#!/usr/bin/env bash
# 内置头像：从 web/assets/frieren 中的动漫插画裁出人物头像（256×256 JPEG），写 web/assets/avatars/<slug>.jpg 与 index.json。
# 依赖 ImageMagick（convert）。用法：bash web/tools/avatars.sh
set -euo pipefail
cd "$(dirname "$0")/../assets"
out=avatars
rm -f "$out"/*.svg "$out"/*.jpg
# slug|名称|源图|裁切（宽x高+x+y）
list='
frieren|芙莉莲|frieren-avatar-generated.jpg|640x640+192+40
frieren-reading|读书的芙莉莲|frieren-empty-state-generated.jpg|320x320+500+150
frieren-sky|晴空下的芙莉莲|frieren-portrait-bluemoon.jpg|420x420+1160+968
frieren-cool|墨镜芙莉莲|frieren-magic-journey.jpg|560x560+952+104
frieren-sofa|沙发上的芙莉莲|frieren-sunset-horizon.jpg|480x480+1328+150
fern-sofa|沙发上的菲伦|frieren-sunset-horizon.jpg|480x480+576+130
blonde-elf|金发精灵|frieren-portrait-bluemoon.jpg|480x480+470+180
gaze|凝视|frieren-sky-landscape.jpg|1000x1000+780+150
staff|法杖|frieren-icon-generated.jpg|900x900+62+62
'
json='['
while IFS='|' read -r slug name src crop; do
  [ -n "$slug" ] || continue
  convert "frieren/$src" -crop "$crop" +repage -resize 256x256^ -gravity center -extent 256x256 -strip -quality 86 "$out/$slug.jpg"
  json+="{\"slug\": \"$slug\", \"name\": \"$name\"}, "
done <<< "$list"
echo "${json%, }]" > "$out/index.json"
