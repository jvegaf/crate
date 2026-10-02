#!/usr/bin/env bash
# Cross-build Crate for Windows (x86_64-pc-windows-gnu) from Linux via the Nix flake.
# Result: src-tauri/target/x86_64-pc-windows-gnu/release/crate-app.exe
# (Installers are Windows-only in the Tauri bundler; use the GitHub release
# workflows for the .msi/.exe installer artifacts.)
set -euo pipefail
cd "$(dirname "$0")/.."

echo "==> workspace deps"
yarn install --frozen-lockfile

echo "==> frontend bundle"
yarn build:vite

echo "==> rust cross build (release, desktop, windows-gnu)"
nix develop .#win-cross --command bash -c \
  "cd src-tauri && rustup target add x86_64-pc-windows-gnu && cargo build --release --features desktop --target x86_64-pc-windows-gnu"

EXE="src-tauri/target/x86_64-pc-windows-gnu/release/crate-app.exe"
file "$EXE"
ls -lh "$EXE"
