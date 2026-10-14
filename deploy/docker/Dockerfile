# PT Studio 镜像：pt-server（内核 + Native 插件）、全部 WASM 插件包、前端、pt-tracker。
# 由 GitHub Actions（.github/workflows/docker.yml）构建并发布到 ghcr.io/<owner>/pt-studio；
# 也可以在仓库根目录手动构建：docker build -f deploy/docker/Dockerfile -t pt-studio .
# 注：pt-tracker 为 AGPL-3.0-only（D16），与其他组件只经 Redis 通信，放在同一镜像中只是为了部署方便。

FROM rust:1-bookworm AS build
SHELL ["/bin/bash", "-c"]
WORKDIR /src
COPY . .
# 在仓库目录中执行：目标装到 rust-toolchain.toml 选定的工具链上
RUN rustup target add wasm32-wasip2
RUN cargo build --release -p pt-server \
 && cargo build --release --manifest-path crates/pt-tracker/Cargo.toml \
 && (cd plugins/wasm && cargo build --release --target wasm32-wasip2)
RUN mkdir -p /out/plugins && for d in plugins/wasm/*/; do \
      id="$(basename "$d")"; [ -f "$d/plugin.toml" ] || continue; [[ "$id" == x-* ]] && continue; \
      target/release/pt-server plugin package --src "$d" \
        --wasm "plugins/wasm/target/wasm32-wasip2/release/pt_wasm_${id//-/_}.wasm" --out /out/plugins; \
    done

FROM node:24-bookworm AS web
WORKDIR /src/web
COPY web/ ./
RUN npm ci && npm run build

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates openssl && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/pt-server /usr/local/bin/pt-server
COPY --from=build /src/crates/pt-tracker/target/release/pt-tracker /usr/local/bin/pt-tracker
COPY --from=build /out/plugins /opt/pt/plugins
COPY --from=web /src/web/dist /opt/pt/web
COPY deploy/site.example.toml deploy/docker/entrypoint.sh /opt/pt/
RUN chmod +x /opt/pt/entrypoint.sh
VOLUME ["/data"]
EXPOSE 31251 31252
ENTRYPOINT ["/opt/pt/entrypoint.sh"]
CMD ["server"]
