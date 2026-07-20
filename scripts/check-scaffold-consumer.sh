#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
temporary="$(mktemp -d "${TMPDIR:-/tmp}/nexora-consumer.XXXXXX")"
trap 'rm -rf "$temporary"' EXIT
target_dir="${CARGO_TARGET_DIR:-$root/target}"
target_dir_path="$target_dir"
manifest_root="$root"
if command -v cygpath >/dev/null 2>&1; then
    target_dir_path="$(cygpath -u "$target_dir")"
    manifest_root="$(cygpath -m "$root")"
fi

cargo build --manifest-path "$root/Cargo.toml" -p nexora --bin nexora
nexora_bin="$target_dir_path/debug/nexora"
if [[ -x "$nexora_bin.exe" ]]; then
    nexora_bin="$nexora_bin.exe"
fi
(
    cd "$temporary"
    "$nexora_bin" create consumer --layout single
)

manifest="$temporary/consumer/Cargo.toml"
sed -i.bak -E \
    "s#nexora = \{[^}]*\}#nexora = { path = \"$manifest_root/crates/nexora\", default-features = false, features = [\"desktop\", \"derive\"] }#" \
    "$manifest"
rm -f "$manifest.bak" "$temporary/consumer/Cargo.lock"

CARGO_TARGET_DIR="$target_dir/scaffold-consumer" \
    cargo check --manifest-path "$manifest"
