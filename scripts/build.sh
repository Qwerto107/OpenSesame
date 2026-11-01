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
  release)
    cargo build --locked --release
    mkdir -p dist
    install -m 755 target/release/opensesame dist/OpenSesame
    ;;
  *) printf '未知构建操作：%s\n' "$action" >&2; exit 1 ;;
esac
