#!/bin/zsh
set -eu

cd "${GRINDLEWALD_REPO:?GRINDLEWALD_REPO is not set}"
exec "${GRINDLEWALD_PNPM:?GRINDLEWALD_PNPM is not set}" tauri dev
