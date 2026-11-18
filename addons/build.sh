#!/usr/bin/env bash
# Builds an addon to WASM and packs it for "Install from file".
# Usage: addons/build.sh csv-export   →   addons/dist/<id>-<version>.zip
# With ZEIMA_SIGN_KEY=<private key file> ZEIMA_SIGN_KEY_ID=<id> the package is signed too.
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
# Sign it when a key is given (docs/addons.md, "Signing and the registry").
if [ -n "${ZEIMA_SIGN_KEY:-}" ]; then
  node ../../scripts/addons.mjs sign "$out/$id-$version.zip" --key "$ZEIMA_SIGN_KEY" --key-id "${ZEIMA_SIGN_KEY_ID:?set ZEIMA_SIGN_KEY_ID}"
fi
echo "$out/$id-$version.zip"
