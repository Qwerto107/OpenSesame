#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
root="$(pwd -P)"
[[ "$(uname -s):$(uname -m)" == Linux:x86_64 ]]
epoch="${SOURCE_DATE_EPOCH:-$(git show -s --format=%ct HEAD)}"
[[ "$epoch" =~ ^[0-9]+$ ]]
bash scripts/build.sh release-web
mkdir -p dist/release
stage="$(mktemp -d "$root/dist/.package-web.XXXXXX")"
cleanup() {
  case "$stage" in "$root/dist/.package-web."*) rm -rf -- "$stage" ;; *) return 1 ;; esac
}
trap cleanup EXIT
name=opensesame-web-linux-amd64
package="$stage/$name"
mkdir -p "$package/licenses"
install -m 755 dist/opensesame-web "$package/opensesame-web"
cp docs/web.md "$package/README.md"
cp web/fonts/LICENSE.txt "$package/licenses/fonts.txt"
python3 .github/scripts/archive.py "$package" "$root/dist/release/$name.tar.gz" "$epoch"
ls -lh "dist/release/$name.tar.gz" "dist/release/$name.tar.gz.sha256"
