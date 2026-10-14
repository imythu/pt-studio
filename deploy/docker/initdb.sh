#!/usr/bin/env bash
# PostgreSQL 容器首次初始化（/docker-entrypoint-initdb.d）：等价于 deploy/postgres/bootstrap.sql
set -euo pipefail
psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname postgres <<SQL
CREATE ROLE pt_admin LOGIN CREATEROLE PASSWORD '${PT_DB_ADMIN_PASSWORD:-change-me-admin}';
CREATE ROLE pt_gateway LOGIN NOINHERIT PASSWORD '${PT_DB_GATEWAY_PASSWORD:-change-me-gateway}';
GRANT pt_gateway TO pt_admin WITH INHERIT FALSE, SET TRUE;
CREATE DATABASE pt OWNER pt_admin;
SQL
psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname pt <<'SQL'
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
REVOKE EXECUTE ON FUNCTION pg_catalog.set_config(text, text, boolean) FROM PUBLIC;
GRANT  EXECUTE ON FUNCTION pg_catalog.set_config(text, text, boolean) TO pt_gateway, pt_admin;
REVOKE USAGE ON LANGUAGE plpgsql FROM PUBLIC;
GRANT  USAGE ON LANGUAGE plpgsql TO pt_gateway, pt_admin WITH GRANT OPTION;
SQL
