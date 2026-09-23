#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(git -C "$script_dir" rev-parse --show-toplevel)"
commit="$(git -C "$repo_root" rev-parse HEAD)"
short_commit="${commit:0:12}"
output_dir="${1:-$repo_root/clients/stateful-codex/eval/artifacts}"
archive="$output_dir/stateful-codex-$short_commit-x86_64-unknown-linux-gnu.tar.gz"
target_dir="${STATEFUL_CODEX_TARGET_DIR:-$repo_root/codex-rs/target}"
staging="$(mktemp -d)"
trap 'rm -rf -- "$staging"' EXIT

if [[ -n "$(git -C "$repo_root" status --short)" ]]; then
  echo "refusing to package a dirty worktree" >&2
  exit 1
fi

mkdir -p "$output_dir" "$staging/package/bin"
cargo build \
  --manifest-path "$repo_root/codex-rs/Cargo.toml" \
  --target-dir "$target_dir" \
  --release \
  -p codex-cli \
  -p codex-code-mode-host

install -m 0755 "$target_dir/release/codex" "$staging/package/bin/codex"
install -m 0755 \
  "$target_dir/release/codex-code-mode-host" \
  "$staging/package/bin/codex-code-mode-host"
printf '%s\n' \
  "{\"layoutVersion\":1,\"version\":\"0.0.0\",\"target\":\"x86_64-unknown-linux-gnu\",\"variant\":\"stateful-codex\",\"entrypoint\":\"bin/codex\",\"sourceCommit\":\"$commit\"}" \
  > "$staging/package/codex-package.json"

tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner \
  -C "$staging/package" -cf - . | gzip -n > "$archive"
sha256sum "$archive" > "$archive.sha256"
printf '%s\n' "$archive"
printf '%s\n' "$archive.sha256"
