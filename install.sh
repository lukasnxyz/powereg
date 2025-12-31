#!/bin/sh

set -e

NAME="powereg"
TARGET_PATH="/usr/local/bin/"

echo "--- Building $NAME in release mode ---"
zig build --release=fast

echo "--- Installing to $TARGET_PATH ---"
sudo cp "zig-out/bin/$NAME" "$TARGET_PATH"

echo "--- Copying powereg.conf to ~/.config/powereg/ ---"
mkdir -p ~/.config/powereg/
cp powereg.conf ~/.config/powereg/powereg.conf

echo "--- You can now run '$NAME' from your terminal. ---"
echo "--- Get started by running 'sudo $NAME --install' to run the daemon. ---"
