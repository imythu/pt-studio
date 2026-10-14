#!/usr/bin/env bash
# 构建 Linux x86_64 发布包：dist/pt-studio-<版本>-linux-x86_64.tar.gz
#
#   deploy/release/build-release.sh            # 构建全部（release）并打包
#   PT_SKIP_BUILD=1 deploy/release/build-release.sh   # 只打包已有的 release 产物
#
# 包内容见 deploy/release/DEPLOY.md §1。二进制只依赖 glibc ≥ 2.34（Debian 12+、Ubuntu 22.04+、RHEL / Rocky 9+）。
set -euo pipefail
shopt -s extglob
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
ARCH="$(uname -m)"
[ "$ARCH" = "x86_64" ] || { echo "this script builds x86_64 packages on an x86_64 host (got $ARCH)" >&2; exit 1; }
NAME="pt-studio-$VERSION-linux-x86_64"
OUT="$ROOT/dist"
STAGE="$OUT/$NAME"
say() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }

if [ "${PT_SKIP_BUILD:-}" != "1" ]; then
  say "building pt-server (release)"
  cargo build --release -p pt-server --manifest-path "$ROOT/Cargo.toml"
  say "building pt-tracker (release)"
  cargo build --release --manifest-path "$ROOT/crates/pt-tracker/Cargo.toml" --bin pt-tracker
  say "building WASM plugins"
  (cd "$ROOT/plugins/wasm" && cargo build --release --target wasm32-wasip2)
  say "building web frontend"
  (cd "$ROOT/web" && { [ -d node_modules ] || npm ci; } && npm run build)
fi

say "staging $STAGE"
rm -rf "$STAGE" && mkdir -p "$STAGE"/{bin,plugins,config,sql,systemd,scripts,licenses,source}
install -m 755 "$ROOT/target/release/pt-server" "$STAGE/bin/"
install -m 755 "$ROOT/crates/pt-tracker/target/release/pt-tracker" "$STAGE/bin/"
strip "$STAGE/bin/pt-server" "$STAGE/bin/pt-tracker" 2>/dev/null || true
for dir in "$ROOT"/plugins/wasm/*/; do
  id="$(basename "$dir")"
  [ -f "$dir/plugin.toml" ] || continue
  case "$id" in x-*) continue ;; esac
  "$STAGE/bin/pt-server" plugin package --src "$dir" \
    --wasm "$ROOT/plugins/wasm/target/wasm32-wasip2/release/pt_wasm_${id//-/_}.wasm" --out "$STAGE/plugins" >/dev/null
done
cp -r "$ROOT/web/dist" "$STAGE/web"
cp "$ROOT/deploy/site.example.toml" "$STAGE/config/"
cp "$ROOT/crates/pt-tracker/tracker.example.toml" "$STAGE/config/"
cp "$ROOT/deploy/postgres/bootstrap.sql" "$STAGE/sql/"
cp "$ROOT"/deploy/release/systemd/*.service "$STAGE/systemd/"
cp "$ROOT"/deploy/release/scripts/*.sh "$STAGE/scripts/"
cp "$ROOT/deploy/release/DEPLOY.md" "$STAGE/"
cp "$ROOT/LICENSE-MIT" "$ROOT/LICENSE-APACHE" "$STAGE/licenses/"
cp "$ROOT/crates/pt-tracker/LICENSE" "$STAGE/licenses/LICENSE-AGPL-3.0-pt-tracker"
# AGPL：pt-tracker 的完整源码随包提供
tar -C "$ROOT/crates" --exclude=target -czf "$STAGE/source/pt-tracker-$VERSION-src.tar.gz" pt-tracker
{
  echo "pt-studio $VERSION"
  echo "commit $(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null || echo unknown)"
  echo "built $(date -u +%Y-%m-%dT%H:%M:%SZ) on $(uname -srm)"
  echo "glibc >= $(objdump -T "$STAGE/bin/pt-server" | grep -oE 'GLIBC_[0-9.]+' | sort -t. -k2 -n -u | tail -1 | sed 's/GLIBC_//')"
} > "$STAGE/VERSION"
(cd "$STAGE" && find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS)

say "checking the staged package"
"$STAGE/bin/pt-server" --version
"$STAGE/bin/pt-tracker" --version
for p in "$STAGE"/plugins/*/; do "$STAGE/bin/pt-server" plugin check "$p" >/dev/null; done
echo "  $(ls "$STAGE/plugins" | wc -l) plugin packages ok"

tar -C "$OUT" -czf "$OUT/$NAME.tar.gz" "$NAME"
(cd "$OUT" && sha256sum "$NAME.tar.gz" > "$NAME.tar.gz.sha256")
say "done: $OUT/$NAME.tar.gz ($(du -h "$OUT/$NAME.tar.gz" | cut -f1))"
