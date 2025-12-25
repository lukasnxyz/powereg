#!/bin/sh

set -e

NAME="powereg"
TARGET_PATH="/usr/local/bin/"

echo "--- Building $NAME in release mode ---"
cargo build --release

echo "--- Installing to $TARGET_PATH ---"
sudo cp "target/release/$NAME" "$TARGET_PATH"

echo "--- You can now run '$NAME' from your terminal. ---"
echo "--- Get started by running 'sudo $NAME --install' to run the daemon. ---"
