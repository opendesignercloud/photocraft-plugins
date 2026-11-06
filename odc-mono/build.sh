#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Open Designer Cloud. Licensed under the Apache License, Version 2.0 (see LICENSE).
# Builds ODC Mono and copies the module to dist/odc-mono.wasm.
# Needs the wasm target once: rustup target add wasm32-unknown-unknown
set -eu
cd "$(dirname "$0")"
cargo build --release --target wasm32-unknown-unknown
mkdir -p dist
cp target/wasm32-unknown-unknown/release/odc_mono.wasm dist/odc-mono.wasm
echo "built dist/odc-mono.wasm ($(wc -c < dist/odc-mono.wasm) bytes)"
