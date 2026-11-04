#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
# Builds ODC Film Look and copies the module to dist/odc-film-look.wasm.
# Needs the wasm target once: rustup target add wasm32-unknown-unknown
set -eu
cd "$(dirname "$0")"
cargo build --release --target wasm32-unknown-unknown
mkdir -p dist
cp target/wasm32-unknown-unknown/release/odc_film_look.wasm dist/odc-film-look.wasm
echo "built dist/odc-film-look.wasm ($(wc -c < dist/odc-film-look.wasm) bytes)"
