#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
action="${1:-release}"

case "$(uname -s)" in
  MINGW*|MSYS*) exec pwsh -NoProfile -File scripts/build.ps1 -Action "$action" ;;
esac
case "$action" in
  fmt) cargo fmt --all -- --check ;;
  lint) cargo clippy --locked --all-targets --all-features -- -D warnings ;;
  check) cargo check --locked ;;
  test) cargo test --locked --lib ;;
  build) cargo build --locked ;;
  qa) cargo build --locked --features qa ;;
  check-web) cargo check --locked --no-default-features --features web --bin opensesame-web ;;
  test-web) cargo test --locked --no-default-features --features web --lib ;;
  release-web)
    [[ "$(uname -s):$(uname -m)" == Linux:x86_64 ]] || { printf '%s\n' 'Web 发行构建仅支持 Linux amd64' >&2; exit 1; }
    cargo build --locked --release --no-default-features --features web --bin opensesame-web
    mkdir -p dist
    install -m 755 "${CARGO_TARGET_DIR:-target}/release/opensesame-web" dist/opensesame-web
    ;;
  release)
    cargo build --locked --release
    mkdir -p dist
    install -m 755 target/release/opensesame dist/OpenSesame
    ;;
  *) printf '未知构建操作：%s\n' "$action" >&2; exit 1 ;;
esac
