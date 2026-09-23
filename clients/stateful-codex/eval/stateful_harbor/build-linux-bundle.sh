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
target="x86_64-unknown-linux-gnu"
v8_profile="ptrcomp_sandbox_release"

if [[ -n "$(git -C "$repo_root" status --short)" ]]; then
  echo "refusing to package a dirty worktree" >&2
  exit 1
fi

mkdir -p "$output_dir" "$staging/package/bin"

v8_version="$(
  python3 "$repo_root/.github/scripts/rusty_v8_bazel.py" \
    resolved-v8-crate-version
)"
v8_release="https://github.com/openai/codex/releases/download/rusty-v8-v$v8_version"
v8_dir="$staging/rusty_v8"
v8_archive="librusty_v8_${v8_profile}_${target}.a.gz"
v8_binding="src_binding_${v8_profile}_${target}.rs"
v8_checksums="rusty_v8_${v8_profile}_${target}.sha256"
trusted_manifests="$repo_root/third_party/v8/rusty_v8_${v8_version//./_}_release_manifests.sha256"
mkdir -p "$v8_dir"
curl -fsSL "$v8_release/$v8_checksums" -o "$v8_dir/$v8_checksums"
expected_manifest_checksum="$(
  grep -F "  $v8_checksums" "$trusted_manifests" | cut -d ' ' -f 1
)"
actual_manifest_checksum="$(sha256sum "$v8_dir/$v8_checksums" | cut -d ' ' -f 1)"
if [[ -z "$expected_manifest_checksum" || \
  "$actual_manifest_checksum" != "$expected_manifest_checksum" ]]; then
  echo "checksum mismatch for $v8_checksums" >&2
  exit 1
fi
curl -fsSL "$v8_release/$v8_archive" -o "$v8_dir/$v8_archive"
curl -fsSL "$v8_release/$v8_binding" -o "$v8_dir/$v8_binding"
(cd "$v8_dir" && tr -d '\r' < "$v8_checksums" | sha256sum --check -)

export RUSTY_V8_ARCHIVE="$v8_dir/$v8_archive"
export RUSTY_V8_SRC_BINDING_PATH="$v8_dir/$v8_binding"
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
