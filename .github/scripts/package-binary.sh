#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
root="$(pwd -P)"
platform="${1:?需要发行平台}"
case "$platform:$(uname -s):$(uname -m)" in
  linux-amd64:Linux:x86_64|linux-arm64:Linux:aarch64) extension=tar.gz ;;
  darwin-amd64:Darwin:x86_64|darwin-arm64:Darwin:arm64)
    extension=zip
    export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-12.0}"
    ;;
  *) printf '发行平台与当前系统或架构不一致：%s\n' "$platform" >&2; exit 1 ;;
esac
epoch="${SOURCE_DATE_EPOCH:-$(git show -s --format=%ct HEAD)}"
[[ "$epoch" =~ ^[0-9]+$ ]]
bash scripts/build.sh release
mkdir -p dist/release
stage="$(mktemp -d "$root/dist/.package-$platform.XXXXXX")"
cleanup() {
  # 暂存目录由本脚本唯一创建，确认位于项目 dist 内再清理。
  case "$stage" in "$root/dist/.package-$platform."*) rm -rf -- "$stage" ;; *) return 1 ;; esac
}
trap cleanup EXIT
name="opensesame-$platform"
package="$stage/$name"
mkdir -p "$package/licenses"
cp docs/binary.md "$package/README.md"
cp assets/fonts/LICENSE.txt "$package/licenses/fonts.txt"

if [[ "$platform" == linux-* ]]; then
  install -m 755 dist/OpenSesame "$package/OpenSesame"
else
  app="$package/OpenSesame.app"
  mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$stage/OpenSesame.iconset"
  install -m 755 dist/OpenSesame "$app/Contents/MacOS/OpenSesame"
  for size in 16 32 128 256 512; do
    sips -z "$size" "$size" assets/icon.png --out "$stage/OpenSesame.iconset/icon_${size}x${size}.png" > /dev/null
    double=$((size * 2))
    sips -z "$double" "$double" assets/icon.png --out "$stage/OpenSesame.iconset/icon_${size}x${size}@2x.png" > /dev/null
  done
  iconutil -c icns "$stage/OpenSesame.iconset" -o "$app/Contents/Resources/OpenSesame.icns"
  version="$(cargo metadata --locked --no-deps --format-version 1 | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "opensesame"))')"
  python3 - "$app/Contents/Info.plist" "$version" <<'PY'
import plistlib
import sys
import os
with open(sys.argv[1], 'wb') as stream:
    plistlib.dump({
        'CFBundleExecutable': 'OpenSesame',
        'CFBundleName': 'OpenSesame',
        'CFBundleDisplayName': 'OpenSesame',
        'CFBundleIdentifier': 'io.github.qwerto107.opensesame',
        'CFBundlePackageType': 'APPL',
        'CFBundleInfoDictionaryVersion': '6.0',
        'CFBundleShortVersionString': sys.argv[2].split('-')[0],
        'CFBundleVersion': sys.argv[2].split('-')[0],
        'CFBundleIconFile': 'OpenSesame.icns',
        'LSMinimumSystemVersion': os.environ['MACOSX_DEPLOYMENT_TARGET'],
        'NSHighResolutionCapable': True,
        'NSLocalNetworkUsageDescription': 'OpenSesame sends TCP/UDP port knocks and checks ports on servers you configure.',
    }, stream, sort_keys=True)
PY
  # 临时签名仅用于测试包；正式 Developer ID 签名和公证需另行配置凭据。
  codesign --force --sign - --timestamp=none "$app"
  codesign --verify --strict "$app"
fi
python3 .github/scripts/archive.py "$package" "$root/dist/release/$name.$extension" "$epoch"
ls -lh "dist/release/$name.$extension" "dist/release/$name.$extension.sha256"
