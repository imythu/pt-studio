#!/usr/bin/env bash
# 备份 PT Studio：PostgreSQL（自定义格式 + 本站角色）、Redis 快照、blob 目录、配置文件，写入一个带时间戳的目录并生成校验和。
#
#   PT_BACKUP_DB_URL=postgres://postgres@127.0.0.1:5432/pt ./scripts/backup.sh /var/backups/pt-studio
#
# 环境变量：
#   PT_ETC（/etc/pt-studio）           site.toml / tracker.toml 所在目录
#   PT_BACKUP_DB_URL                   有权读取全部 schema 的连接串（超级用户，或带 pg_read_all_data 的角色）；
#                                      默认取 site.toml 的 database.admin_url（管理角色读不到插件私有表时会失败）
#   PT_BACKUP_REDIS_URL                默认取 site.toml 的 [redis.main] url；设为 skip 跳过
#   PT_BACKUP_KEEP（14）               保留最近几份备份，0 = 不清理
#   PSQL_ARGS                          连接 postgres 库导出角色时的额外参数（如 "-h 127.0.0.1 -p 5432 -U postgres"）
#
# 备份包含密钥（site.toml 中的 secret_key 等）与用户数据，请放在受控位置并加密传输。
set -euo pipefail
OUT_ROOT="${1:?usage: backup.sh <backup root dir>}"
ETC="${PT_ETC:-/etc/pt-studio}"
SITE="$ETC/site.toml"
[ -f "$SITE" ] || { echo "site.toml not found: $SITE" >&2; exit 1; }
toml() { # toml <section> <key>：读取简单的 key = "value"
  awk -v sec="[$1]" -v key="$2" '
    /^\[/ { insec = ($0 == sec) }
    insec && $1 == key { sub(/^[^=]*=[ \t]*"/, ""); sub(/".*$/, ""); print; exit }' "$SITE"
}
DB_URL="${PT_BACKUP_DB_URL:-$(toml database admin_url)}"
REDIS_URL="${PT_BACKUP_REDIS_URL:-$(toml redis.main url)}"
BLOB_DIR="$(toml blob dir)"
PREFIX="$(toml database role_prefix)"; PREFIX="${PREFIX:-pt_}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
DEST="$OUT_ROOT/pt-backup-$STAMP"
umask 077
mkdir -p "$DEST"
say() { printf '==> %s\n' "$*"; }

say "database → db.dump"
pg_dump --format=custom --no-password --file "$DEST/db.dump" "$DB_URL"
say "roles (${PREFIX}*) → roles.sql"
# shellcheck disable=SC2086
pg_dumpall ${PSQL_ARGS:-} --roles-only --no-role-passwords 2>/dev/null \
  | grep -E "ROLE \"?${PREFIX}|GRANT .*${PREFIX}" > "$DEST/roles.sql" || {
  # 没有超级用户时导出不了角色：恢复到原集群时角色已存在，可以忽略
  echo "-- roles not exported (needs superuser); restore into a cluster where they exist" > "$DEST/roles.sql"
}
if [ "$REDIS_URL" != "skip" ] && [ -n "$REDIS_URL" ]; then
  say "redis → redis.rdb"
  redis-cli -u "$REDIS_URL" --rdb "$DEST/redis.rdb" >/dev/null
fi
if [ -n "$BLOB_DIR" ] && [ -d "$BLOB_DIR" ]; then
  say "blob $BLOB_DIR → blob.tar.gz"
  tar -C "$BLOB_DIR" -czf "$DEST/blob.tar.gz" .
fi
say "config → config/"
mkdir -p "$DEST/config"
cp "$ETC"/*.toml "$DEST/config/" 2>/dev/null || true
[ -f "$ETC/plugin-signing.pub" ] && cp "$ETC/plugin-signing.pub" "$DEST/config/"
{
  echo "created $STAMP"
  echo "host $(hostname)"
  echo "role_prefix $PREFIX"
  pg_restore --list "$DEST/db.dump" | grep -c "TABLE DATA" | sed 's/^/tables /'
} > "$DEST/MANIFEST"
(cd "$DEST" && find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS)
say "done: $DEST ($(du -sh "$DEST" | cut -f1))"

KEEP="${PT_BACKUP_KEEP:-14}"
if [ "$KEEP" -gt 0 ]; then
  ls -1d "$OUT_ROOT"/pt-backup-* 2>/dev/null | sort | head -n -"$KEEP" | while read -r old; do
    say "removing old backup $old"
    rm -rf -- "$old"
  done
fi
