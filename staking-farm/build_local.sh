#!/bin/bash
set -e

RUSTFLAGS='-C link-arg=-s' cargo +stable build --target wasm32-unknown-unknown --release
cd ..
cp target/wasm32-unknown-unknown/release/staking_farm.wasm ./res/staking_farm_local.wasm
