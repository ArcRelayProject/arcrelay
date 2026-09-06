#!/usr/bin/env bash
set -euo pipefail

manifest_path="${1:?usage: verify-updater-assets.sh MANIFEST TARGETS}"
target_list="${2:?usage: verify-updater-assets.sh MANIFEST TARGETS}"

: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"
: "${RELEASE_ID:?RELEASE_ID is required}"
: "${GH_TOKEN:?GH_TOKEN is required}"

for command in base64 curl gh jq minisign python3; do
  command -v "$command" >/dev/null
done

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

jq -er '.plugins.updater.pubkey' tauri.conf.json | base64 --decode > "$work_dir/public.key"
gh api "repos/$GITHUB_REPOSITORY/releases/$RELEASE_ID/assets" > "$work_dir/release-assets.json"

IFS=',' read -r -a targets <<< "$target_list"
for target in "${targets[@]}"; do
  asset_url="$(jq -er --arg target "$target" '.platforms[$target].url' "$manifest_path")"
  asset_name="$(python3 - "$asset_url" <<'PY'
import sys
from urllib.parse import unquote, urlparse

path = urlparse(sys.argv[1]).path
name = unquote(path.rsplit('/', 1)[-1])
if not name or name in {'.', '..'} or '/' in name or '\\' in name:
    raise SystemExit('invalid updater asset name')
print(name)
PY
)"
  asset_api="$(jq -er --arg name "$asset_name" '.[] | select(.name == $name) | .url' "$work_dir/release-assets.json")"
  signature_path="$work_dir/$target.sig"
  payload_path="$work_dir/$target.update"

  jq -er --arg target "$target" '.platforms[$target].signature' "$manifest_path" \
    | base64 --decode > "$signature_path"
  gh api -H 'Accept: application/octet-stream' "$asset_api" > "$payload_path"
  minisign -Vm "$payload_path" -x "$signature_path" -p "$work_dir/public.key"
done
