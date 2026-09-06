#!/usr/bin/env bash
set +x
set -euo pipefail

: "${RUNNER_TEMP:?RUNNER_TEMP is required}"
: "${GITHUB_ENV:?GITHUB_ENV is required}"
: "${APPLE_CERTIFICATE:?APPLE_CERTIFICATE is required}"
: "${APPLE_CERTIFICATE_PASSWORD:?APPLE_CERTIFICATE_PASSWORD is required}"
: "${APPLE_SIGNING_IDENTITY:?APPLE_SIGNING_IDENTITY is required}"

keychain="$RUNNER_TEMP/arcrelay-signing.keychain-db"
certificate="$RUNNER_TEMP/arcrelay-signing.p12"
keychain_password="$(openssl rand -base64 32)"

cleanup_failure() {
  status=$?
  trap - EXIT
  rm -f "$certificate"
  if [[ $status -ne 0 ]]; then
    security delete-keychain "$keychain" >/dev/null 2>&1 || true
  fi
  exit "$status"
}
trap cleanup_failure EXIT

printf '%s' "$APPLE_CERTIFICATE" | base64 --decode > "$certificate"
security create-keychain -p "$keychain_password" "$keychain"
security set-keychain-settings -lut 7200 "$keychain"
security unlock-keychain -p "$keychain_password" "$keychain"
security import "$certificate" \
  -k "$keychain" \
  -P "$APPLE_CERTIFICATE_PASSWORD" \
  -A -t cert -f pkcs12
security set-key-partition-list \
  -S 'apple-tool:,apple:,codesign:' -s \
  -k "$keychain_password" "$keychain" >/dev/null
security list-keychains -d user -s "$keychain" $(security list-keychains -d user | tr -d '"')
security default-keychain -d user -s "$keychain"
security find-identity -v -p codesigning "$keychain" | grep -F "$APPLE_SIGNING_IDENTITY" >/dev/null

rm -f "$certificate"
printf 'ARCRELAY_SIGNING_KEYCHAIN=%s\n' "$keychain" >> "$GITHUB_ENV"
