#!/bin/sh

# TODO: check if cargo installed
# TODO: check if libudev intsalled
# TODO: check if system is using systemd

set -e

NAME="powereg"
TARGET_PATH="/usr/local/bin/"

echo "--- Building $NAME in release mode ---"
cargo build --release

echo "--- Installing to $TARGET_PATH ---"
sudo cp "target/release/$NAME" "$TARGET_PATH"

echo "--- Copying powereg.conf to ~/.config/powereg/ ---"
mkdir -p ~/.config/powereg/
cp powereg.toml ~/.config/powereg/config.toml

echo "--- You can now run '$NAME' from your terminal. ---"
echo "--- Get started by running 'sudo $NAME --install' to run the daemon. ---"
