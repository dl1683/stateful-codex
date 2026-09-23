#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
if repo_root="$(git -C "$script_dir" rev-parse --show-toplevel 2>/dev/null)"; then
  commit="$(git -C "$repo_root" rev-parse HEAD)"
  if [[ -n "$(git -C "$repo_root" status --short)" ]]; then
    echo "refusing to package a dirty worktree" >&2
    exit 1
  fi
else
  repo_root="$(cd -- "$script_dir/../../../.." && pwd)"
  commit="${STATEFUL_CODEX_SOURCE_COMMIT:-}"
  if [[ ! "$commit" =~ ^[0-9a-f]{40}$ ]]; then
    echo "STATEFUL_CODEX_SOURCE_COMMIT must identify the archived source" >&2
    exit 1
  fi
fi
short_commit="${commit:0:12}"
output_dir="${1:-$repo_root/clients/stateful-codex/eval/artifacts}"
archive="$output_dir/stateful-codex-$short_commit-x86_64-unknown-linux-gnu.tar.gz"
target_dir="${STATEFUL_CODEX_TARGET_DIR:-$repo_root/codex-rs/target}"
staging="$(mktemp -d)"
trap 'rm -rf -- "$staging"' EXIT
target="x86_64-unknown-linux-gnu"
v8_profile="ptrcomp_sandbox_release"

mkdir -p "$output_dir" "$staging/package/bin" "$staging/package/lib"

mapfile -t v8_versions < <(
  awk '
    $0 == "name = \"v8\"" {
      getline
      if ($1 == "version" && $2 == "=") {
        gsub(/\"/, "", $3)
        print $3
      }
    }
  ' "$repo_root/codex-rs/Cargo.lock" | sort -u
)
if [[ "${#v8_versions[@]}" -ne 1 ]]; then
  echo "expected exactly one resolved v8 version" >&2
  exit 1
fi
v8_version="${v8_versions[0]}"
v8_release="https://github.com/openai/codex/releases/download/rusty-v8-v$v8_version"
v8_dir="${STATEFUL_CODEX_V8_CACHE_DIR:-$target_dir/rusty_v8}"
v8_archive="librusty_v8_${v8_profile}_${target}.a.gz"
v8_binding="src_binding_${v8_profile}_${target}.rs"
v8_checksums="rusty_v8_${v8_profile}_${target}.sha256"
trusted_manifests="$repo_root/third_party/v8/rusty_v8_${v8_version//./_}_release_manifests.sha256"
mkdir -p "$v8_dir"
curl_options=(
  --fail
  --silent
  --show-error
  --location
  --retry 3
  --retry-all-errors
  --connect-timeout 15
  --max-time 300
)

download_v8_artifact() {
  local name="$1"
  local partial="$v8_dir/$name.partial"
  curl "${curl_options[@]}" "$v8_release/$name" -o "$partial"
  mv -f "$partial" "$v8_dir/$name"
}

ensure_v8_artifact() {
  local name="$1"
  local expected
  local actual=""
  expected="$(grep -F "  $name" "$v8_dir/$v8_checksums" | cut -d ' ' -f 1)"
  if [[ -z "$expected" ]]; then
    echo "missing checksum for $name" >&2
    exit 1
  fi
  if [[ -f "$v8_dir/$name" ]]; then
    actual="$(sha256sum "$v8_dir/$name" | cut -d ' ' -f 1)"
  fi
  if [[ "$actual" != "$expected" ]]; then
    download_v8_artifact "$name"
  fi
}

download_v8_artifact "$v8_checksums"
expected_manifest_checksum="$(
  grep -F "  $v8_checksums" "$trusted_manifests" | cut -d ' ' -f 1
)"
actual_manifest_checksum="$(sha256sum "$v8_dir/$v8_checksums" | cut -d ' ' -f 1)"
if [[ -z "$expected_manifest_checksum" || \
  "$actual_manifest_checksum" != "$expected_manifest_checksum" ]]; then
  echo "checksum mismatch for $v8_checksums" >&2
  exit 1
fi
ensure_v8_artifact "$v8_archive"
ensure_v8_artifact "$v8_binding"
(cd "$v8_dir" && tr -d '\r' < "$v8_checksums" | sha256sum --check -)

export RUSTY_V8_ARCHIVE="$v8_dir/$v8_archive"
export RUSTY_V8_SRC_BINDING_PATH="$v8_dir/$v8_binding"
(
  cd "$repo_root/codex-rs"
  cargo build \
    --target-dir "$target_dir" \
    --release \
    -p codex-cli \
    -p codex-code-mode-host
)

install -m 0755 "$target_dir/release/codex" "$staging/package/bin/codex.real"
install -m 0755 \
  "$target_dir/release/codex-code-mode-host" \
  "$staging/package/bin/codex-code-mode-host.real"
strip --strip-unneeded "$staging/package/bin/codex.real"
strip --strip-unneeded "$staging/package/bin/codex-code-mode-host.real"

for library in libssl.so.1.1 libcrypto.so.1.1; do
  library_path="$(ldconfig -p | awk -v name="$library" '$1 == name { print $NF; exit }')"
  if [[ -z "$library_path" ]]; then
    echo "missing required runtime library: $library" >&2
    exit 1
  fi
  cp -L -- "$library_path" "$staging/package/lib/$library"
  chmod 0644 "$staging/package/lib/$library"
done

for executable in codex codex-code-mode-host; do
  cat > "$staging/package/bin/$executable" <<EOF
#!/bin/sh
script_path="\$(readlink -f -- "\$0")"
script_dir="\$(dirname -- "\$script_path")"
export LD_LIBRARY_PATH="\$script_dir/../lib\${LD_LIBRARY_PATH:+:\$LD_LIBRARY_PATH}"
exec "\$script_dir/$executable.real" "\$@"
EOF
  chmod 0755 "$staging/package/bin/$executable"
done
printf '%s\n' \
  "{\"layoutVersion\":1,\"version\":\"0.0.0\",\"target\":\"x86_64-unknown-linux-gnu\",\"variant\":\"stateful-codex\",\"entrypoint\":\"bin/codex\",\"sourceCommit\":\"$commit\",\"bundledRuntimeLibraries\":[\"libssl.so.1.1\",\"libcrypto.so.1.1\"]}" \
  > "$staging/package/codex-package.json"

tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner \
  -C "$staging/package" -cf - . | gzip -n > "$archive"
archive_digest="$(sha256sum "$archive" | cut -d ' ' -f 1)"
printf '%s  %s\n' "$archive_digest" "$(basename -- "$archive")" \
  > "$archive.sha256"
printf '%s\n' "$archive"
printf '%s\n' "$archive.sha256"
