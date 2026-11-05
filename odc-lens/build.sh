#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
# Builds ODC Lens and copies the module to dist/odc-lens.wasm.
# Needs the wasm target once: rustup target add wasm32-unknown-unknown
set -eu
cd "$(dirname "$0")"
cargo build --release --target wasm32-unknown-unknown
mkdir -p dist
cp target/wasm32-unknown-unknown/release/odc_lens.wasm dist/odc-lens.wasm
echo "built dist/odc-lens.wasm ($(wc -c < dist/odc-lens.wasm) bytes)"
