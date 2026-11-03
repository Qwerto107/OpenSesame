#!/usr/bin/env bash
set -euo pipefail

image="ghcr.io/${GITHUB_REPOSITORY,,}"
tags=("$image:sha-${GITHUB_SHA}")

if [[ "$GITHUB_EVENT_NAME" == "push" && "$GITHUB_REF" == refs/tags/* ]]; then
  version="${GITHUB_REF#refs/tags/v}"
  # 仅正式语义版本更新 latest，预发布版本只更新自身标签。
  if [[ ${#version} -gt 128 || ! "$GITHUB_REF" =~ ^refs/tags/v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$ ]]; then
    printf '版本标签必须为 vX.Y.Z 或 vX.Y.Z-预发布标识\n' >&2
    exit 1
  fi
  tags+=("$image:$version")
  if [[ "$version" != *-* ]]; then tags+=("$image:latest"); fi
elif [[ "$GITHUB_EVENT_NAME" == "workflow_dispatch" ]]; then
  if [[ "$GITHUB_REF" != "refs/heads/main" ]]; then
    printf '手动发布请选择 main 分支\n' >&2
    exit 1
  fi
else
  printf '当前事件不允许发布镜像\n' >&2
  exit 1
fi

{
  printf 'tags<<OPENSESAME_IMAGE_TAGS\n'
  printf '%s\n' "${tags[@]}"
  printf 'OPENSESAME_IMAGE_TAGS\n'
} >> "${GITHUB_OUTPUT:?缺少 GITHUB_OUTPUT}"
