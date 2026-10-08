#!/usr/bin/env sh
set -eu
cd "$(dirname "$0")/.."
(cd ui && npm ci && npm run build)
cargo test --workspace
cargo build --release

