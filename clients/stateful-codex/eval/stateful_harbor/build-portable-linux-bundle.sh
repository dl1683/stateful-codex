#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(git -C "$script_dir" rev-parse --show-toplevel)"
commit="$(git -C "$repo_root" rev-parse HEAD)"
output_dir="${1:-$repo_root/clients/stateful-codex/eval/artifacts}"
context_dir="$(mktemp -d)"
trap 'rm -rf -- "$context_dir"' EXIT

if [[ -n "$(git -C "$repo_root" status --short)" ]]; then
  echo "refusing to build a dirty worktree" >&2
  exit 1
fi
if ! docker version >/dev/null 2>&1; then
  echo "Docker is required to build the portable Linux bundle" >&2
  exit 1
fi

mkdir -p "$output_dir"
git -C "$repo_root" archive --format=tar HEAD | tar -xf - -C "$context_dir"

docker_output_dir="$output_dir"
if command -v cygpath >/dev/null 2>&1; then
  docker_output_dir="$(cygpath --windows "$output_dir")"
fi
docker buildx build \
  --build-arg "SOURCE_COMMIT=$commit" \
  --file clients/stateful-codex/eval/stateful_harbor/Dockerfile.bundle \
  --output "type=local,dest=$docker_output_dir" \
  --progress plain \
  "$context_dir"
