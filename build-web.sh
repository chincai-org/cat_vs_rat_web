#!/usr/bin/env bash
set -euo pipefail

rustup target add wasm32-unknown-unknown
RUSTFLAGS="-C link-arg=--allow-undefined" cargo build --release --target wasm32-unknown-unknown -p cat-vs-rat

# JS runtime and storage plugin. Downloaded once into web/vendor; commit them to pin versions.
mkdir -p web/vendor
fetch() { [ -f "web/vendor/$1" ] || curl -fsSL "$2" -o "web/vendor/$1"; }
fetch mq_js_bundle.js  https://not-fl3.github.io/miniquad-samples/mq_js_bundle.js
fetch sapp_jsutils.js  https://raw.githubusercontent.com/not-fl3/sapp-jsutils/master/js/sapp_jsutils.js
fetch quad-storage.js  https://raw.githubusercontent.com/optozorax/quad-storage/master/js/quad-storage.js

rm -rf dist && mkdir -p dist
cp target/wasm32-unknown-unknown/release/cat_vs_rat.wasm dist/
cp web/index.html dist/
cp web/vendor/*.js dist/
cp -r Assets dist/Assets
rm -f dist/Assets/*.mp3   # the web build uses the .ogg files

echo "Built into dist/. Try: python3 -m http.server -d dist 8000"
