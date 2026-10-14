#!/usr/bin/env bash
# 初始化 PostgreSQL：创建管理角色、网关角色与数据库，并收紧函数与语言权限（等价于 sql/bootstrap.sql）。
# 以能连接到 PostgreSQL 的超级用户执行，例如：
#   sudo -u postgres env PT_DB_ADMIN_PASSWORD=… PT_DB_GATEWAY_PASSWORD=… ./scripts/init-db.sh
# 可选：PT_DB_NAME（默认 pt）、PSQL_ARGS（如 "-h 127.0.0.1 -p 5432"）
set -euo pipefail
: "${PT_DB_ADMIN_PASSWORD:?set PT_DB_ADMIN_PASSWORD}"
: "${PT_DB_GATEWAY_PASSWORD:?set PT_DB_GATEWAY_PASSWORD}"
DB="${PT_DB_NAME:-pt}"
# shellcheck disable=SC2086
psql ${PSQL_ARGS:-} -v ON_ERROR_STOP=1 -d postgres <<SQL
DO \$\$ BEGIN
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'pt_admin') THEN
    CREATE ROLE pt_admin LOGIN CREATEROLE PASSWORD '${PT_DB_ADMIN_PASSWORD}';
  END IF;
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'pt_gateway') THEN
    CREATE ROLE pt_gateway LOGIN NOINHERIT PASSWORD '${PT_DB_GATEWAY_PASSWORD}';
  END IF;
END \$\$;
GRANT pt_gateway TO pt_admin WITH INHERIT FALSE, SET TRUE;
SQL
if [ "$(psql ${PSQL_ARGS:-} -qAt -d postgres -c "SELECT 1 FROM pg_database WHERE datname = '$DB'")" != "1" ]; then
  psql ${PSQL_ARGS:-} -v ON_ERROR_STOP=1 -d postgres -c "CREATE DATABASE $DB OWNER pt_admin"
fi
psql ${PSQL_ARGS:-} -v ON_ERROR_STOP=1 -d "$DB" <<'SQL'
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
REVOKE EXECUTE ON FUNCTION pg_catalog.set_config(text, text, boolean) FROM PUBLIC;
GRANT  EXECUTE ON FUNCTION pg_catalog.set_config(text, text, boolean) TO pt_gateway, pt_admin;
REVOKE USAGE ON LANGUAGE plpgsql FROM PUBLIC;
GRANT  USAGE ON LANGUAGE plpgsql TO pt_gateway, pt_admin WITH GRANT OPTION;
SQL
echo "ok: database $DB, roles pt_admin / pt_gateway"
