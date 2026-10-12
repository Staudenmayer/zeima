#!/usr/bin/env bash
# Builds an addon to WASM and packs it for "Install from file".
# Usage: addons/build.sh csv-export   →   addons/dist/<id>-<version>.zip
set -euo pipefail
cd "$(dirname "$0")/$1"
cargo build --release --target wasm32-unknown-unknown
wasm=$(grep -m1 -E '^wasm *=' addon.toml | sed -E 's/.*"(.*)"/\1/')
id=$(grep -m1 -E '^id *=' addon.toml | sed -E 's/.*"(.*)"/\1/')
version=$(grep -m1 -E '^version *=' addon.toml | sed -E 's/.*"(.*)"/\1/')
out=../dist
mkdir -p "$out" target/pack
cp addon.toml target/pack/
cp "target/wasm32-unknown-unknown/release/$wasm" "target/pack/$wasm"
rm -f "$out/$id-$version.zip"
(cd target/pack && zip -q "../../$out/$id-$version.zip" addon.toml "$wasm")
echo "$out/$id-$version.zip"
