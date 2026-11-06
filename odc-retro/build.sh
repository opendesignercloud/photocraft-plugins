#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Open Designer Cloud. Licensed under the Apache License, Version 2.0 (see LICENSE).
# Builds ODC Retro and copies the module to dist/odc-retro.wasm.
# Needs the wasm target once: rustup target add wasm32-unknown-unknown
set -eu
cd "$(dirname "$0")"
cargo build --release --target wasm32-unknown-unknown
mkdir -p dist
cp target/wasm32-unknown-unknown/release/odc_retro.wasm dist/odc-retro.wasm
echo "built dist/odc-retro.wasm ($(wc -c < dist/odc-retro.wasm) bytes)"
