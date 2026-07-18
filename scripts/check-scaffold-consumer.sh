#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
temporary="$(mktemp -d "${TMPDIR:-/tmp}/nexora-consumer.XXXXXX")"
trap 'rm -rf "$temporary"' EXIT

cargo build --manifest-path "$root/Cargo.toml" -p nexora --bin nexora
(
    cd "$temporary"
    "$root/target/debug/nexora" create consumer --layout single
)

manifest="$temporary/consumer/Cargo.toml"
sed -i.bak -E \
    "s#nexora = \{[^}]*\}#nexora = { path = \"$root/crates/nexora\", default-features = false, features = [\"desktop\", \"derive\"] }#" \
    "$manifest"
rm -f "$manifest.bak" "$temporary/consumer/Cargo.lock"

CARGO_TARGET_DIR="$root/target/scaffold-consumer" \
    cargo check --manifest-path "$manifest"
