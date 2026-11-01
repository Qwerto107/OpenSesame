#!/usr/bin/env bash
set -euo pipefail

tag="${GITHUB_REF_NAME:?缺少版本标签}"
if [[ ${#tag} -gt 100 || ! "$tag" =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$ ]]; then
  printf '%s\n' '版本标签格式无效' >&2
  exit 1
fi
: "${GH_REPO:?缺少仓库名称}" "${GITHUB_SHA:?缺少源码提交}" "${GITHUB_STEP_SUMMARY:?缺少运行摘要路径}"

cd release-assets
archives=(opensesame-windows-amd64.zip opensesame-linux-amd64.tar.gz opensesame-linux-arm64.tar.gz opensesame-darwin-amd64.zip opensesame-darwin-arm64.zip)
for archive in "${archives[@]}"; do
  test -s "$archive"
  test -s "$archive.sha256"
  # 仅接受预期文件名，避免校验文件意外引用压缩包以外的路径。
  checksum="$(cat "$archive.sha256")"
  if [[ ! "$checksum" =~ ^[0-9a-f]{64}\ \ [a-z0-9.-]+$ || "${checksum:66}" != "$archive" ]]; then
    printf '校验文件格式无效：%s\n' "$archive.sha256" >&2
    exit 1
  fi
  sha256sum -c "$archive.sha256"
done

prerelease=false
latest=true
if [[ "$tag" == *-* ]]; then prerelease=true; latest=false; fi
printf '%s\n\n' 'OpenSesame 是跨平台 TCP / UDP 端口敲门客户端，提供 Windows x64、Linux x64 / ARM64、macOS Intel / Apple Silicon 版本。Windows 与 macOS 使用 ZIP，Linux 使用 TAR.GZ，每份附件附 SHA-256。macOS app 使用临时签名，尚未进行 Developer ID 签名和公证。' > release-notes.md
printf '源码提交：%s\n\n' "$GITHUB_SHA" >> release-notes.md
printf '[二进制启动说明](https://github.com/%s/blob/%s/docs/binary.md) · [配置参考](https://github.com/%s/blob/%s/docs/configuration.md) · [发布流程](https://github.com/%s/blob/%s/docs/releasing.md)\n' \
  "$GH_REPO" "$tag" "$GH_REPO" "$tag" "$GH_REPO" "$tag" >> release-notes.md

# 先建草稿；核对 GitHub 上的实际附件后才公开，重试时保留已有附件。
if ! gh release view "$tag" --json tagName > /dev/null 2>&1; then
  gh release create "$tag" --verify-tag --draft --title "$tag" --generate-notes --notes-file release-notes.md
fi
existing_assets="$(gh release view "$tag" --json assets --jq '.assets[].name')"
for archive in "${archives[@]}"; do
  for asset in "$archive" "$archive.sha256"; do
    if ! grep -Fxq "$asset" <<< "$existing_assets"; then gh release upload "$tag" "$asset"; fi
  done
done
verification_directory="$(mktemp -d)"
cleanup_verification() {
  for archive in "${archives[@]}"; do
    rm -f -- "$verification_directory/$archive" "$verification_directory/$archive.sha256"
  done
  rmdir -- "$verification_directory"
}
trap cleanup_verification EXIT
for archive in "${archives[@]}"; do
  gh release download "$tag" --dir "$verification_directory" --pattern "$archive" --pattern "$archive.sha256"
  test -s "$verification_directory/$archive"
  cmp "$archive.sha256" "$verification_directory/$archive.sha256"
  (cd "$verification_directory" && sha256sum -c "$archive.sha256")
done
gh release edit "$tag" --draft=false --prerelease="$prerelease" --latest="$latest"
printf 'Release: https://github.com/%s/releases/tag/%s\n' "$GH_REPO" "$tag" >> "$GITHUB_STEP_SUMMARY"
