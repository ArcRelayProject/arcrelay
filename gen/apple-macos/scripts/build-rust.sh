#!/bin/bash
set -euo pipefail

# GUI-launched Xcode does not inherit the shell PATH used by Terminal/Codex.
# Load rustup's environment when available, then add the standard Rust and
# Homebrew locations explicitly so Command-R behaves the same as a CLI build.
if [[ -f "${HOME}/.cargo/env" ]]; then
  # shellcheck disable=SC1091
  source "${HOME}/.cargo/env"
fi
export PATH="${HOME}/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:${PATH:-/usr/bin:/bin:/usr/sbin:/sbin}"

configuration="${1:-Debug}"
archs="${2:-arm64}"
project_root="$(cd "${PROJECT_DIR}/../.." && pwd)"
workspace_root="$(cd "${project_root}/.." && pwd)"
cargo_target_dir="${workspace_root}/target/xcode-macos"
profile="debug"

require_tool() {
  local tool="$1"
  if ! command -v "${tool}" >/dev/null 2>&1; then
    echo "error: Required tool '${tool}' was not found while building ArcRelay." >&2
    echo "error: PATH=${PATH}" >&2
    exit 127
  fi
}

require_tool cargo

case "${configuration}" in
  Release|release) profile="release" ;;
esac

ensure_vite_server() {
  local dev_url="http://localhost:1420"
  local vite_log="${TMPDIR:-/tmp}/arcrelay-vite.log"

  if /usr/bin/curl --silent --fail --max-time 1 "${dev_url}" >/dev/null 2>&1; then
    return
  fi

  require_tool npm
  echo "Starting ArcRelay Vite development server at ${dev_url}..."
  /usr/bin/nohup npm run dev >"${vite_log}" 2>&1 </dev/null &

  for _ in {1..100}; do
    if /usr/bin/curl --silent --fail --max-time 1 "${dev_url}" >/dev/null 2>&1; then
      echo "ArcRelay Vite development server is ready."
      return
    fi
    /bin/sleep 0.1
  done

  echo "error: ArcRelay's Vite development server did not start at ${dev_url}." >&2
  echo "error: Vite log: ${vite_log}" >&2
  /usr/bin/tail -n 30 "${vite_log}" >&2 || true
  exit 1
}

if [[ "${profile}" == "debug" ]]; then
  ensure_vite_server
fi

target_for_arch() {
  case "$1" in
    arm64) echo "aarch64-apple-darwin" ;;
    x86_64) echo "x86_64-apple-darwin" ;;
    *) echo "Unsupported macOS architecture: $1" >&2; return 1 ;;
  esac
}

binary_paths=()
cd "${project_root}"

for arch in ${archs}; do
  target="$(target_for_arch "${arch}")"
  if [[ "${profile}" == "release" ]]; then
    require_tool npm
    CARGO_TARGET_DIR="${cargo_target_dir}" npm run tauri -- build --no-bundle --target "${target}"
  else
    CARGO_TARGET_DIR="${cargo_target_dir}" cargo build --package arcrelay-desktop --target "${target}"
  fi
  binary_paths+=("${cargo_target_dir}/${target}/${profile}/arcrelay-desktop")
done

destination="${TARGET_BUILD_DIR}/${EXECUTABLE_PATH}"
mkdir -p "$(dirname "${destination}")"
rm -f "${destination}"

if [[ ${#binary_paths[@]} -eq 1 ]]; then
  cp "${binary_paths[0]}" "${destination}"
else
  /usr/bin/lipo -create "${binary_paths[@]}" -output "${destination}"
fi

chmod +x "${destination}"

# Xcode builds bypass Tauri's bundle pipeline, so stage the same on-demand
# screenshot sidecars into Contents/MacOS and sign them before the app itself.
require_tool npm
if [[ ${#binary_paths[@]} -eq 1 ]]; then
  sniptra_target="${target}"
else
  sniptra_target="universal-apple-darwin"
fi

SNIPTRA_SIDECAR_TARGET="${sniptra_target}" \
  CARGO_TARGET_DIR="${workspace_root}/target" \
  npm run prepare:sniptra

signing_identity="${EXPANDED_CODE_SIGN_IDENTITY:--}"
if [[ -z "${signing_identity}" ]]; then
  signing_identity="-"
fi
for sidecar_name in sniptra sniptra-ocr-worker; do
  sidecar_source="${project_root}/binaries/${sidecar_name}-${sniptra_target}"
  sidecar_destination="$(dirname "${destination}")/${sidecar_name}"
  cp "${sidecar_source}" "${sidecar_destination}"
  chmod +x "${sidecar_destination}"
  /usr/bin/codesign --force --sign "${signing_identity}" "${sidecar_destination}"
done

resources_dir="${TARGET_BUILD_DIR}/${UNLOCALIZED_RESOURCES_FOLDER_PATH}"
mkdir -p "${resources_dir}"
cp "${project_root}/icons/icon-macos-1024.png" "${resources_dir}/icon-macos-1024.png"
cp "${project_root}/icons/icon-drag-preview.png" "${resources_dir}/icon-drag-preview.png"
