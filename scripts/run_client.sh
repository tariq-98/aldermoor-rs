#!/bin/sh
# Run from any directory; all assets are read from the catalogue in this checkout.
set -eu
cd "$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cargo build --release -p client
exec target/release/client "$@"
