# 源码构建与发行镜像共用运行层；CI 直接复用已校验的 Linux amd64 程序。
FROM rust:1.99.1-bookworm AS backend
WORKDIR /src
COPY Cargo.toml Cargo.lock build.rs rust-toolchain.toml ./
COPY src/ ./src/
COPY i18n/ ./i18n/
COPY web/ ./web/
ARG GITHUB_SHA
RUN cargo build --locked --release --no-default-features --features web --bin opensesame-web

FROM debian:bookworm-slim AS runtime-files
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates tzdata libgcc-s1 \
    && groupadd -g 65532 opensesame && useradd -u 65532 -g opensesame -M opensesame \
    && mkdir /data /runtime-tmp /runtime-libs \
    && chown 65532:65532 /data && chmod 1777 /runtime-tmp \
    && cp --parents /lib/x86_64-linux-gnu/libc.so.6 /lib/x86_64-linux-gnu/libm.so.6 \
       /lib/x86_64-linux-gnu/libgcc_s.so.1 /lib64/ld-linux-x86-64.so.2 /runtime-libs/ \
    && rm -rf /var/lib/apt/lists/*

FROM scratch AS runtime
COPY --from=runtime-files /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=runtime-files /usr/share/zoneinfo /usr/share/zoneinfo
COPY --from=runtime-files /etc/passwd /etc/passwd
COPY --from=runtime-files /etc/group /etc/group
# Rust GNU 发行程序需要 glibc、数学库、动态加载器及 libgcc。
COPY --from=runtime-files /runtime-libs/ /
COPY --from=runtime-files --chown=65532:65532 /data /data
COPY --from=runtime-files --chmod=1777 /runtime-tmp /tmp
ENV OPENSESAME_WEB_LISTEN=0.0.0.0:18010 \
    OPENSESAME_WEB_DB=/data/opensesame-web.sqlite3
USER 65532:65532
WORKDIR /data
EXPOSE 18010
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD ["/opensesame-web", "healthcheck"]
ENTRYPOINT ["/opensesame-web"]

FROM debian:bookworm-slim AS release-binary
COPY .release/binaries/opensesame-web-linux-amd64/opensesame-web /opensesame-web
RUN chmod 0755 /opensesame-web

FROM runtime AS release
COPY --from=release-binary /opensesame-web /opensesame-web

# 默认目标支持本地从源码构建。
FROM runtime AS standalone
COPY --from=backend /src/target/release/opensesame-web /opensesame-web
