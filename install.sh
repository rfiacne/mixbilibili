#!/usr/bin/env bash
set -euo pipefail

echo "Building mixbilibili in release mode..."
cargo build --release

echo "Installing to /usr/local/bin/mixbilibili..."
sudo cp target/release/mixbilibili /usr/local/bin/mixbilibili

echo "Done. Version: $(mixbilibili --version)"
