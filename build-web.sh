#!/usr/bin/env bash
set -euo pipefail

rustup target add wasm32-unknown-unknown
# Size-first codegen for the web build; desktop keeps opt-level 3.
CARGO_PROFILE_RELEASE_OPT_LEVEL=s \
  RUSTFLAGS="-C link-arg=--allow-undefined" \
  cargo build --release --target wasm32-unknown-unknown -p cat-vs-rat

# JS runtime and storage plugin. Downloaded once into web/vendor; commit them to pin versions.
mkdir -p web/vendor
fetch() { [ -f "web/vendor/$1" ] || curl -fsSL "$2" -o "web/vendor/$1"; }
fetch mq_js_bundle.js  https://not-fl3.github.io/miniquad-samples/mq_js_bundle.js
fetch sapp_jsutils.js  https://raw.githubusercontent.com/not-fl3/sapp-jsutils/master/js/sapp_jsutils.js
fetch quad-storage.js  https://raw.githubusercontent.com/optozorax/quad-storage/master/js/quad-storage.js

rm -rf dist && mkdir -p dist
WASM=target/wasm32-unknown-unknown/release/cat_vs_rat.wasm
# Binaryen is optional; it shrinks the wasm further when installed. List the
# features Rust emits explicitly: newer Binaryen's --all-features also enables
# compact imports, which browsers reject.
if command -v wasm-opt >/dev/null 2>&1 \
  && wasm-opt \
    --enable-bulk-memory \
    --enable-nontrapping-float-to-int \
    --enable-sign-ext \
    --enable-mutable-globals \
    --enable-reference-types \
    --enable-multivalue \
    -Oz "$WASM" -o dist/cat_vs_rat.wasm; then
  :
else
  cp "$WASM" dist/
fi
cp web/index.html dist/
cp web/vendor/*.js dist/
cp -r Assets dist/Assets
rm -f dist/Assets/*.mp3   # the web build uses the .ogg files

echo "Built into dist/. Try: python3 -m http.server -d dist 8000"
