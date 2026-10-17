#!/usr/bin/env bash
# 从 backup.sh 的备份目录恢复 PostgreSQL 与 blob 目录（先停止 pt-server 与 pt-tracker）。
#
#   PT_RESTORE_DB_URL=postgres://postgres@127.0.0.1:5432/pt ./scripts/restore.sh /var/backups/pt-studio/pt-backup-20261017T030000Z
#
# 环境变量：
#   PT_RESTORE_DB_URL      目标数据库（超级用户连接；数据库须已存在且为空，可先用 init-db.sh 创建）
#   PT_RESTORE_BLOB_DIR    blob 目录（默认取备份中 site.toml 的 [blob] dir）
#   PSQL_ARGS              连接 postgres 库应用 roles.sql 的额外参数
#   PT_RESTORE_FORCE=1     目标库不为空时仍然继续（会先清空目标库中本站的 schema）
#
# Redis：备份中的 redis.rdb 只在需要找回尚未入库的流量记录时使用——停止 Redis 后替换其数据目录中的 dump.rdb 再启动。
# tracker 的 swarm 会在客户端下一次 announce 时自动重建。
set -euo pipefail
SRC="${1:?usage: restore.sh <backup dir>}"
: "${PT_RESTORE_DB_URL:?set PT_RESTORE_DB_URL}"
say() { printf '==> %s\n' "$*"; }
[ -f "$SRC/db.dump" ] || { echo "$SRC/db.dump not found" >&2; exit 1; }
say "verifying checksums"
(cd "$SRC" && sha256sum --quiet -c SHA256SUMS)

if [ -s "$SRC/roles.sql" ] && ! grep -q "^-- roles not exported" "$SRC/roles.sql"; then
  say "roles"
  # 已存在的角色报错可以忽略
  # shellcheck disable=SC2086
  PGOPTIONS="-c client_min_messages=warning" psql ${PSQL_ARGS:-} -q -d postgres -f "$SRC/roles.sql" 2>&1 | grep -v "already exists" || true
fi

existing="$(psql -qAt "$PT_RESTORE_DB_URL" -c "SELECT count(*) FROM pg_namespace WHERE nspname NOT LIKE 'pg\_%' AND nspname NOT IN ('public', 'information_schema')")"
if [ "$existing" != "0" ]; then
  [ "${PT_RESTORE_FORCE:-}" = "1" ] || { echo "target database is not empty ($existing schemas); set PT_RESTORE_FORCE=1 to replace it" >&2; exit 1; }
  say "dropping $existing existing schemas"
  psql -q -v ON_ERROR_STOP=1 "$PT_RESTORE_DB_URL" <<'SQL'
DO $$ DECLARE s text; BEGIN
  FOR s IN SELECT nspname FROM pg_namespace WHERE nspname NOT LIKE 'pg\_%' AND nspname NOT IN ('public', 'information_schema') LOOP
    EXECUTE format('DROP SCHEMA %I CASCADE', s);
  END LOOP;
END $$;
SQL
fi
say "database"
pg_restore --exit-on-error --no-password --dbname "$PT_RESTORE_DB_URL" "$SRC/db.dump"

BLOB="${PT_RESTORE_BLOB_DIR:-$(awk '/^\[blob\]/{f=1;next} /^\[/{f=0} f && $1=="dir"{sub(/^[^=]*=[ \t]*"/,""); sub(/".*$/,""); print; exit}' "$SRC/config/site.toml" 2>/dev/null)}"
if [ -f "$SRC/blob.tar.gz" ] && [ -n "$BLOB" ]; then
  say "blob → $BLOB"
  mkdir -p "$BLOB"
  tar -C "$BLOB" -xzf "$SRC/blob.tar.gz"
fi
say "done. Check site.toml (secret keys must match the backup: $SRC/config/site.toml), then start pt-server and pt-tracker."
