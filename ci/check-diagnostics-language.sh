#!/usr/bin/env bash
set -euo pipefail

# Developer-facing diagnostics are part of the public API for contributors.
# Localized UI catalogs and intentional multilingual fixtures are excluded.
common_globs=(
  --glob '!**/target/**'
  --glob '!**/node_modules/**'
  --glob '!**/assets/**'
  --glob '!**/generated/**'
  --glob '!**/*.g.dart'
  --glob '!**/l10n/**'
  --glob '!**/locales/**'
)
source_globs=(--glob '*.{rs,ts,tsx,js,svelte,dart,proto,toml,yaml,yml,sh}')
if (($#)); then
  scan_roots=("$@")
else
  scan_roots=(arcrelay-* ci)
fi
failed=0

check() {
  local description=$1
  local pattern=$2
  shift 2
  local output
  output=$(rg -n -U -P "$pattern" "${source_globs[@]}" "${common_globs[@]}" "$@" || true)
  if [[ -n "$output" ]]; then
    printf '%s\n%s\n' "$description" "$output" >&2
    failed=1
  fi
}

check \
  'Non-English source comments found:' \
  '(?://[/!]?[^\n]*\p{Han}|^\s*(?:/\*+|\*)[^\n]*\p{Han})' \
  "${scan_roots[@]}"

check \
  'Non-English Rust diagnostics found:' \
  '(?s)(?:#\[error\s*\(\s*|Err\s*\(\s*|map_err\s*\(\s*\|[^|]*\|\s*|ok_or(?:_else)?\s*\(\s*(?:\|\|\s*)?|panic!\s*\(\s*|bail!\s*\(\s*|anyhow!\s*\(\s*)(?:(?:[A-Za-z_][A-Za-z0-9_]*::)*[A-Za-z_][A-Za-z0-9_]*\s*\(\s*){0,3}(?:format!\s*\(\s*)?"[^"\n]*\p{Han}' \
  "${scan_roots[@]}"

check \
  'Non-English Rust ensure messages found:' \
  '(?s)ensure!\s*\((?:(?!\);)[\s\S]){0,1024}"[^"\n]*\p{Han}' \
  "${scan_roots[@]}"

check \
  'Non-English Rust expect messages found:' \
  '(?s)\.expect\s*\(\s*"[^"\n]*\p{Han}' \
  "${scan_roots[@]}"

check \
  'Non-English validation field names found:' \
  '(?s)\bvalidate_[A-Za-z0-9_]+\s*\(\s*"[^"\n]*\p{Han}' \
  "${scan_roots[@]}"

check \
  'Non-English log messages found:' \
  '(?s)(?:tracing::|\b(?:trace|debug|info|warn|error)!|println!|eprintln!|console\.(?:log|debug|info|warn|error))[^;]{0,2048}\p{Han}' \
  "${scan_roots[@]}"

frontend_errors=$(rg -n -U -P '(?s)\bthrow\b[^;]{0,2048}\p{Han}' \
  "${source_globs[@]}" "${common_globs[@]}" "${scan_roots[@]}" \
  | rg -v '(?:\btr|uiTranslate|translate)\(' || true)
if [[ -n "$frontend_errors" ]]; then
  printf '%s\n%s\n' 'Non-English frontend exception messages found:' "$frontend_errors" >&2
  failed=1
fi

if ((failed)); then
  printf '%s\n' \
    'Keep diagnostics and comments in English. Put user-facing translations in locale catalogs.' >&2
  exit 1
fi
