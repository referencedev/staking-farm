#!/bin/bash
set -e

RUSTFLAGS='-C link-arg=-s' cargo build --target wasm32-unknown-unknown --release
mkdir -p ../res
cp target/wasm32-unknown-unknown/release/mock_receiver.wasm ../res/
