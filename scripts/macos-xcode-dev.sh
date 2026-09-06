#!/bin/bash
set -euo pipefail

project_root="$(cd "$(dirname "$0")/.." && pwd)"

"${project_root}/scripts/macos-xcode-generate.sh"

cd "${project_root}"
vite_pid=""

if lsof -nP -iTCP:1420 -sTCP:LISTEN >/dev/null 2>&1; then
  echo "Vite is already listening on port 1420; reusing the existing server."
else
  npm run dev &
  vite_pid=$!
fi

cleanup() {
  if [[ -n "${vite_pid}" ]]; then
    kill "${vite_pid}" 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

open "${project_root}/gen/apple-macos/ArcRelay.xcodeproj"
echo "Vite is running. Press Cmd+R in Xcode to build and launch ArcRelay."

if [[ -n "${vite_pid}" ]]; then
  wait "${vite_pid}"
fi
