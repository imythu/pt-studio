#!/usr/bin/env bash
# 生成 /etc/pt-studio/site.toml 与 tracker.toml（已存在则不覆盖）。密钥随机生成，生成后请妥善备份 site.toml。
# 环境变量（括号内为默认值）：
#   PT_BASE_URL（http://localhost:31251）  站点对外地址
#   PT_TRACKER_URL（http://localhost:31252） tracker 对外地址（种子中写入 <地址>/announce/<passkey>）
#   PT_BIND（0.0.0.0:31251）、PT_TRACKER_BIND（0.0.0.0:31252）
#   PT_DB_HOST（127.0.0.1:5432）、PT_DB_NAME（pt）、PT_DB_ADMIN_PASSWORD、PT_DB_GATEWAY_PASSWORD（必填）
#   PT_REDIS_URL（redis://127.0.0.1:6379/0）
#   PT_MEILI_URL（http://127.0.0.1:7700）、PT_MEILI_KEY（Meilisearch master key，必填；不用搜索时随便填）
#   PT_HOME（安装目录，默认本脚本的上一级）、PT_ETC（/etc/pt-studio）、PT_DATA（/var/lib/pt-studio）
set -euo pipefail
HOME_DIR="${PT_HOME:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
ETC="${PT_ETC:-/etc/pt-studio}"
DATA="${PT_DATA:-/var/lib/pt-studio}"
: "${PT_DB_ADMIN_PASSWORD:?set PT_DB_ADMIN_PASSWORD}"
: "${PT_DB_GATEWAY_PASSWORD:?set PT_DB_GATEWAY_PASSWORD}"
: "${PT_MEILI_KEY:?set PT_MEILI_KEY}"
command -v openssl >/dev/null || { echo "openssl is required" >&2; exit 1; }
mkdir -p "$ETC" "$DATA/blob"
DB_HOST="${PT_DB_HOST:-127.0.0.1:5432}"
DB="${PT_DB_NAME:-pt}"
REDIS="${PT_REDIS_URL:-redis://127.0.0.1:6379/0}"
TRACKER="${PT_TRACKER_URL:-http://localhost:31252}"
# 发布包带签名公钥（插件包已签名）：生产模式，只加载受信公钥签名的插件包
if [ -f "$HOME_DIR/config/plugin-signing.pub" ]; then
  MODE_LINE="mode = \"prod\""
  UNSIGNED_LINE="allow_unsigned = false\ntrusted_keys = [\"$HOME_DIR/config/plugin-signing.pub\"]"
else
  MODE_LINE="mode = \"dev\"                           \# 插件包未签名，加载需要 dev 模式"
  UNSIGNED_LINE="allow_unsigned = true"
fi
if [ -f "$ETC/site.toml" ]; then
  echo "keep existing $ETC/site.toml"
else
  sed -e "s#http://localhost:31251#${PT_BASE_URL:-http://localhost:31251}#" \
      -e "s#bind = \"0.0.0.0:31251\"#bind = \"${PT_BIND:-0.0.0.0:31251}\"#" \
      -e "s#web_dir = \"web/dist\"#web_dir = \"$HOME_DIR/web\"#" \
      -e "s#^mode = \"dev\" .*#$MODE_LINE#" \
      -e "s#^allow_unsigned = true .*#$UNSIGNED_LINE#" \
      -e "s#pt_gateway:change-me-gateway@127.0.0.1:5432/pt#pt_gateway:${PT_DB_GATEWAY_PASSWORD}@$DB_HOST/$DB#" \
      -e "s#pt_admin:change-me-admin@127.0.0.1:5432/pt#pt_admin:${PT_DB_ADMIN_PASSWORD}@$DB_HOST/$DB#" \
      -e "s#redis://127.0.0.1:6379/0#$REDIS#" \
      -e "s#dir = \"data/blob\"#dir = \"$DATA/blob\"#" \
      -e "s#dir = \"dist/plugins\"#dir = \"$HOME_DIR/plugins\"#" \
      -e "s#@SECRET_HEX_32#$(openssl rand -hex 32)#" \
      -e "s#@SECRET_BASE64_32#$(openssl rand -base64 32)#" \
      -e "s#http://127.0.0.1:7700#${PT_MEILI_URL:-http://127.0.0.1:7700}#" \
      -e "s#@MEILI_MASTER_KEY#${PT_MEILI_KEY}#" \
      -e "s#http://localhost:31252/announce#$TRACKER/announce#" \
      "$HOME_DIR/config/site.example.toml" > "$ETC/site.toml"
  chmod 640 "$ETC/site.toml"
  echo "wrote $ETC/site.toml"
fi
if [ -f "$ETC/tracker.toml" ]; then
  echo "keep existing $ETC/tracker.toml"
else
  printf 'node_id = "%s"\nbind = "%s"\nredis_url = "%s"\n# 在反向代理之后时改为 true（从 X-Forwarded-For 取客户端 IP）\ntrust_forwarded = false\n' \
    "$(hostname -s 2>/dev/null || echo tracker-1)" "${PT_TRACKER_BIND:-0.0.0.0:31252}" "$REDIS" > "$ETC/tracker.toml"
  chmod 640 "$ETC/tracker.toml"
  echo "wrote $ETC/tracker.toml"
fi
if id pt-studio >/dev/null 2>&1; then
  chown -R pt-studio:pt-studio "$DATA"
  chgrp pt-studio "$ETC"/site.toml "$ETC"/tracker.toml
fi
"$HOME_DIR/bin/pt-server" --config "$ETC/site.toml" check >/dev/null && echo "ok: configuration and plugins check passed"
