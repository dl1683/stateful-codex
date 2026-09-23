#!/usr/bin/env bash
set -euo pipefail

bundle="${1:?usage: preflight-linux-bundle.sh BUNDLE [TASK_IMAGE]}"
task_image="${2:-alexgshaw/fix-git:20260403}"
sidecar="$bundle.sha256"

if [[ ! -f "$bundle" || ! -f "$sidecar" ]]; then
  echo "bundle and SHA-256 sidecar are required" >&2
  exit 1
fi
expected_digest="$(cut -d ' ' -f 1 "$sidecar")"
actual_digest="$(sha256sum "$bundle" | cut -d ' ' -f 1)"
if [[ ! "$expected_digest" =~ ^[0-9a-f]{64}$ || \
  "$actual_digest" != "$expected_digest" ]]; then
  echo "bundle SHA-256 mismatch" >&2
  exit 1
fi
printf '%s: OK\n' "$(basename -- "$bundle")"

container="$(docker create --entrypoint sh "$task_image" -c '
  set -eu
  mkdir -p /tmp/stateful-codex-preflight
  tar -xzf /tmp/stateful-codex.tar.gz -C /tmp/stateful-codex-preflight
  if ldd /tmp/stateful-codex-preflight/bin/codex | grep -F "not found"; then
    exit 1
  fi
  if ldd /tmp/stateful-codex-preflight/bin/codex-code-mode-host | grep -F "not found"; then
    exit 1
  fi
  mkdir -p /tmp/stateful-codex-preflight-links
  ln -s /tmp/stateful-codex-preflight/bin/codex \
    /tmp/stateful-codex-preflight-links/codex
  ln -s /tmp/stateful-codex-preflight/bin/codex-code-mode-host \
    /tmp/stateful-codex-preflight-links/codex-code-mode-host
  /tmp/stateful-codex-preflight-links/codex --version
  /tmp/stateful-codex-preflight-links/codex-code-mode-host --help >/dev/null
  test -s /tmp/stateful-codex-preflight/codex-package.json
')"
trap 'docker rm --force "$container" >/dev/null 2>&1 || true' EXIT
docker cp "$bundle" "$container:/tmp/stateful-codex.tar.gz"
docker start --attach "$container"
