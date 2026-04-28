#!/usr/bin/env bash
set -euo pipefail

echo "Building mixbilibili in release mode..."
cargo build --release

echo "Installing to /usr/local/bin/mix..."
sudo cp target/release/mixbilibili /usr/local/bin/mix

echo "Done. Version: $(mix --version)"
