#!/usr/bin/env bash
set -euo pipefail

if [[ -n "${ARCRELAY_SIGNING_KEYCHAIN:-}" ]]; then
  security delete-keychain "$ARCRELAY_SIGNING_KEYCHAIN" >/dev/null 2>&1 || true
fi
