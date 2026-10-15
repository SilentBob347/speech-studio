#!/bin/bash
# Builds the experimental Linux x86-64 Speech Studio: a .deb and an AppImage. Engines, CUDA libraries and
# models are downloaded in the app ("First run"); the bundle carries only the frontend.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
staging="$root/desktop/src-tauri/staging"

cd "$root"
npm --prefix frontend ci --no-audit --no-fund
npm --prefix frontend run build
cargo test --workspace

rm -rf "$staging"
mkdir -p "$staging/frontend"
cp -r frontend/dist "$staging/frontend/dist"
echo "[OK] bundle staged in $staging"

npm --prefix desktop ci --no-audit --no-fund
cd desktop
npm exec tauri build -- --config src-tauri/tauri.linux-release.conf.json --bundles deb,appimage

find src-tauri/target/release/bundle \( -name '*.deb' -o -name '*.AppImage' \)
