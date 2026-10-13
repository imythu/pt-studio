#!/usr/bin/env bash
# 本机一键运行 PT Studio 站点（无 Docker）：检查 / 启动依赖、初始化数据库、构建服务端与插件与前端、生成配置、迁移、启动。
#
#   deploy/dev/run-site.sh            # 构建并启动（默认 release 构建，首次较慢）
#   PT_PROFILE=debug deploy/dev/run-site.sh
#   PT_SKIP_BUILD=1 deploy/dev/run-site.sh   # 已构建过，直接启动
#
# 依赖：PostgreSQL ≥ 16、Redis ≥ 7.4、Node.js ≥ 20、Rust（含 wasm32-wasip2 目标）；Meilisearch 可选（没有时搜索不可用）。
# 端口可用环境变量覆盖：PT_PG_PORT（55432）、PT_REDIS_PORT（56379）、PT_MEILI_PORT（57700）、PT_HTTP_PORT（8080）；
# 已在运行的 Meilisearch 用 PT_MEILI_KEY 给出 master key。
# 数据与配置放在 .dev/site/；删除该目录并 DROP DATABASE ptsite 即可从头开始。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DEV="$ROOT/.dev/site"
PROFILE="${PT_PROFILE:-release}"
PG_PORT="${PT_PG_PORT:-55432}"
REDIS_PORT="${PT_REDIS_PORT:-56379}"
MEILI_PORT="${PT_MEILI_PORT:-57700}"
HTTP_PORT="${PT_HTTP_PORT:-8080}"
PG_SUPER="${PT_PG_SUPERUSER:-postgres}"
# 与测试库共用 PostgreSQL 集群时，角色名不能冲突（测试会删除 pt_plugin_* 角色）：本脚本使用独立的前缀与库名
PREFIX="${PT_ROLE_PREFIX:-ptsite_}"
DB="${PT_DB_NAME:-ptsite}"
MEILI_KEY_FILE="$DEV/meili.key"
mkdir -p "$DEV"/{blob,plugins,meili}

say() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "missing $1 ($2)"; }

need psql "PostgreSQL client"
need redis-cli "Redis"
need cargo "Rust toolchain"
need node "Node.js ≥ 20"
need openssl "for generating secrets"

psql_su() { psql -h 127.0.0.1 -p "$PG_PORT" -U "$PG_SUPER" -v ON_ERROR_STOP=1 -qAt "$@"; }

# ---------- 依赖服务 ----------
say "PostgreSQL on :$PG_PORT"
if ! pg_isready -h 127.0.0.1 -p "$PG_PORT" >/dev/null 2>&1; then
  if [ -d "$ROOT/.dev/pg" ]; then
    PGBIN="$(ls -d /usr/lib/postgresql/*/bin 2>/dev/null | sort -V | tail -1)"
    su postgres -c "$PGBIN/pg_ctl -D $ROOT/.dev/pg -o '-p $PG_PORT -k /tmp' -l $ROOT/.dev/pg.log start" >/dev/null
    sleep 2
  fi
  pg_isready -h 127.0.0.1 -p "$PG_PORT" >/dev/null 2>&1 || die "PostgreSQL is not running on port $PG_PORT (see docs/dev/README.md §1.2)"
fi

say "Redis on :$REDIS_PORT"
if ! redis-cli -p "$REDIS_PORT" ping >/dev/null 2>&1; then
  redis-server --port "$REDIS_PORT" --daemonize yes --save '' --appendonly no >/dev/null
  sleep 1
fi

# 已有 Meilisearch 在运行时用 PT_MEILI_KEY 给出它的 master key；否则由本脚本启动并生成 key
if [ -n "${PT_MEILI_KEY:-}" ]; then
  MEILI_KEY="$PT_MEILI_KEY"
else
  [ -f "$MEILI_KEY_FILE" ] || openssl rand -hex 16 > "$MEILI_KEY_FILE"
  MEILI_KEY="$(cat "$MEILI_KEY_FILE")"
fi
MEILI_BIN="${PT_MEILI_BIN:-$(command -v meilisearch || echo "$ROOT/.dev/bin/meilisearch")}"
if [ -x "$MEILI_BIN" ]; then
  if ! curl -fs "http://127.0.0.1:$MEILI_PORT/health" >/dev/null 2>&1; then
    say "Meilisearch on :$MEILI_PORT"
    nohup "$MEILI_BIN" --http-addr "127.0.0.1:$MEILI_PORT" --master-key "$MEILI_KEY" \
      --db-path "$DEV/meili/data.ms" --dump-dir "$DEV/meili/dumps" --no-analytics > "$DEV/meili.log" 2>&1 &
    sleep 2
  fi
else
  say "Meilisearch not found — search will be unavailable (set PT_MEILI_BIN)"
fi

# ---------- 数据库 ----------
# 等价于 deploy/postgres/bootstrap.sql，只是角色与库名换成本脚本的前缀与库名，且可重复执行
if [ "$(psql_su -d postgres -c "SELECT 1 FROM pg_database WHERE datname = '$DB'")" != "1" ]; then
  say "creating database $DB with roles ${PREFIX}admin / ${PREFIX}gateway"
  psql_su -d postgres <<SQL
DO \$\$ BEGIN
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = '${PREFIX}admin') THEN
    CREATE ROLE ${PREFIX}admin LOGIN CREATEROLE PASSWORD 'change-me-admin';
  END IF;
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = '${PREFIX}gateway') THEN
    CREATE ROLE ${PREFIX}gateway LOGIN NOINHERIT PASSWORD 'change-me-gateway';
  END IF;
END \$\$;
GRANT ${PREFIX}gateway TO ${PREFIX}admin WITH INHERIT FALSE, SET TRUE;
CREATE DATABASE $DB OWNER ${PREFIX}admin;
SQL
  awk 'f{print} /^-- 以下在目标数据库中执行/{f=1}' "$ROOT/deploy/postgres/bootstrap.sql" \
    | sed -e "s/pt_admin/${PREFIX}admin/g" -e "s/pt_gateway/${PREFIX}gateway/g" | psql_su -d "$DB"
fi

# ---------- 构建 ----------
if [ "${PT_SKIP_BUILD:-}" != "1" ]; then
  say "building pt-server ($PROFILE)"
  if [ "$PROFILE" = "release" ]; then
    cargo build --release -p pt-server --manifest-path "$ROOT/Cargo.toml"
  else
    cargo build -p pt-server --manifest-path "$ROOT/Cargo.toml"
  fi
  say "building WASM plugins"
  rustup target list --installed 2>/dev/null | grep -q wasm32-wasip2 || rustup target add wasm32-wasip2
  (cd "$ROOT/plugins/wasm" && cargo build --release --target wasm32-wasip2)
  say "building web frontend"
  (cd "$ROOT/web" && { [ -d node_modules ] || npm ci; } && npm run build)
fi
SERVER="$ROOT/target/$PROFILE/pt-server"
[ -x "$SERVER" ] || die "$SERVER not built (run without PT_SKIP_BUILD)"

say "packaging WASM plugins into $DEV/plugins"
rm -rf "$DEV/plugins" && mkdir -p "$DEV/plugins"
for dir in "$ROOT"/plugins/wasm/*/; do
  id="$(basename "$dir")"
  [ -f "$dir/plugin.toml" ] || continue
  case "$id" in x-*) continue ;; esac   # 测试插件
  wasm="$ROOT/plugins/wasm/target/wasm32-wasip2/release/pt_wasm_${id//-/_}.wasm"
  "$SERVER" plugin package --src "$dir" --wasm "$wasm" --out "$DEV/plugins" >/dev/null
done

# ---------- 配置 ----------
CFG="$DEV/site.toml"
if [ ! -f "$CFG" ]; then
  say "writing $CFG"
  sed -e "s#http://localhost:8080#http://localhost:$HTTP_PORT#" \
      -e "s#0.0.0.0:8080#0.0.0.0:$HTTP_PORT#" \
      -e "s#web_dir = \"web/dist\"#web_dir = \"$ROOT/web/dist\"#" \
      -e "s#pt_gateway:change-me-gateway@127.0.0.1:5432/pt#${PREFIX}gateway:change-me-gateway@127.0.0.1:$PG_PORT/$DB#" \
      -e "s#pt_admin:change-me-admin@127.0.0.1:5432/pt#${PREFIX}admin:change-me-admin@127.0.0.1:$PG_PORT/$DB#" \
      -e "s#^pool_size = 20#role_prefix = \"$PREFIX\"\npool_size = 20#" \
      -e "s#redis://127.0.0.1:6379/0#redis://127.0.0.1:$REDIS_PORT/1#" \
      -e "s#dir = \"data/blob\"#dir = \"$DEV/blob\"#" \
      -e "s#dir = \"dist/plugins\"#dir = \"$DEV/plugins\"#" \
      -e "s#@SECRET_HEX_32#$(openssl rand -hex 32)#" \
      -e "s#@SECRET_BASE64_32#$(openssl rand -base64 32)#" \
      -e "s#http://127.0.0.1:7700#http://127.0.0.1:$MEILI_PORT#" \
      -e "s#@MEILI_MASTER_KEY#$MEILI_KEY#" \
      "$ROOT/deploy/site.example.toml" > "$CFG"
fi

say "checking plugins and migrating"
"$SERVER" --config "$CFG" check
"$SERVER" --config "$CFG" migrate

cat <<EOF

  PT Studio:  http://localhost:$HTTP_PORT
  1. 打开站点注册一个账号；
  2. 授予管理员：$SERVER --config $CFG user grant-role <用户名> admin
  3. 后台：http://localhost:$HTTP_PORT/admin

EOF
exec "$SERVER" --config "$CFG" serve
