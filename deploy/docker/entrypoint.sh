#!/usr/bin/env bash
# 容器入口：server（首次启动生成 /data/site.toml，迁移后启动）或 tracker。
# 环境变量：PT_BASE_URL、PT_TRACKER_URL、PT_DB_HOST、PT_DB_NAME、PT_DB_ADMIN_PASSWORD、PT_DB_GATEWAY_PASSWORD、
#           PT_REDIS_URL、PT_MEILI_URL、PT_MEILI_KEY
set -euo pipefail
mode="${1:-server}"
CFG=/data/site.toml
REDIS="${PT_REDIS_URL:-redis://redis:6379/0}"

case "$mode" in
  server)
    if [ ! -f "$CFG" ]; then
      mkdir -p /data/blob
      DB="${PT_DB_NAME:-pt}"
      HOST="${PT_DB_HOST:-postgres:5432}"
      sed -e "s#http://localhost:8080#${PT_BASE_URL:-http://localhost:8080}#" \
          -e "s#web_dir = \"web/dist\"#web_dir = \"/opt/pt/web\"#" \
          -e "s#pt_gateway:change-me-gateway@127.0.0.1:5432/pt#pt_gateway:${PT_DB_GATEWAY_PASSWORD:-change-me-gateway}@$HOST/$DB#" \
          -e "s#pt_admin:change-me-admin@127.0.0.1:5432/pt#pt_admin:${PT_DB_ADMIN_PASSWORD:-change-me-admin}@$HOST/$DB#" \
          -e "s#redis://127.0.0.1:6379/0#$REDIS#" \
          -e "s#dir = \"data/blob\"#dir = \"/data/blob\"#" \
          -e "s#dir = \"dist/plugins\"#dir = \"/opt/pt/plugins\"#" \
          -e "s#@SECRET_HEX_32#$(openssl rand -hex 32)#" \
          -e "s#@SECRET_BASE64_32#$(openssl rand -base64 32)#" \
          -e "s#http://127.0.0.1:7700#${PT_MEILI_URL:-http://meilisearch:7700}#" \
          -e "s#@MEILI_MASTER_KEY#${PT_MEILI_KEY:-}#" \
          -e "s#http://localhost:8081/announce#${PT_TRACKER_URL:-http://localhost:8081}/announce#" \
          /opt/pt/site.example.toml > "$CFG"
      echo "generated $CFG (keep it: it holds the site secrets)"
    fi
    pt-server --config "$CFG" migrate
    exec pt-server --config "$CFG" serve
    ;;
  tracker)
    printf 'node_id = "%s"\nbind = "0.0.0.0:8081"\nredis_url = "%s"\ntrust_forwarded = %s\n' \
      "${PT_TRACKER_NODE:-tracker-1}" "$REDIS" "${PT_TRUST_FORWARDED:-false}" > /tmp/tracker.toml
    exec pt-tracker /tmp/tracker.toml
    ;;
  *)
    # 其他命令原样执行，如：docker compose exec server pt-server --config /data/site.toml user grant-role alice admin
    exec "$@"
    ;;
esac
