#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
# Builds ODC Tone and copies the module to dist/odc-tone.wasm.
# Needs the wasm target once: rustup target add wasm32-unknown-unknown
set -eu
cd "$(dirname "$0")"
cargo build --release --target wasm32-unknown-unknown
mkdir -p dist
cp target/wasm32-unknown-unknown/release/odc_tone.wasm dist/odc-tone.wasm
echo "built dist/odc-tone.wasm ($(wc -c < dist/odc-tone.wasm) bytes)"
