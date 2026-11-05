#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
# Builds ODC Glow and copies the module to dist/odc-glow.wasm.
# Needs the wasm target once: rustup target add wasm32-unknown-unknown
set -eu
cd "$(dirname "$0")"
cargo build --release --target wasm32-unknown-unknown
mkdir -p dist
cp target/wasm32-unknown-unknown/release/odc_glow.wasm dist/odc-glow.wasm
echo "built dist/odc-glow.wasm ($(wc -c < dist/odc-glow.wasm) bytes)"
