#!/usr/bin/env bash
set -euo pipefail

bundle="${1:?usage: preflight-linux-bundle.sh BUNDLE [TASK_IMAGE]}"
task_image="${2:-alexgshaw/fix-git:20260403}"
sidecar="$bundle.sha256"

if [[ ! -f "$bundle" || ! -f "$sidecar" ]]; then
  echo "bundle and SHA-256 sidecar are required" >&2
  exit 1
fi
(cd -- "$(dirname -- "$bundle")" && sha256sum --check "$(basename -- "$sidecar")")

container="$(docker create --entrypoint sh "$task_image" -c '
  set -eu
  mkdir -p /tmp/stateful-codex-preflight
  tar -xzf /tmp/stateful-codex.tar.gz -C /tmp/stateful-codex-preflight
  /tmp/stateful-codex-preflight/bin/codex --version
  /tmp/stateful-codex-preflight/bin/codex-code-mode-host --help >/dev/null
  test -s /tmp/stateful-codex-preflight/codex-package.json
')"
trap 'docker rm --force "$container" >/dev/null 2>&1 || true' EXIT
docker cp "$bundle" "$container:/tmp/stateful-codex.tar.gz"
docker start --attach "$container"
