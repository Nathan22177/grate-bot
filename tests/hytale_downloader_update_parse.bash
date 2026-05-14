#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$repo_root/deploy/hytale-downloader-update.sh"

assert_extracts_version() {
  local input="$1"
  local expected="$2"
  local actual

  actual="$(extract_server_version <<<"$input")"
  if [[ "$actual" != "$expected" ]]; then
    printf 'expected %q, got %q from input %q\n' "$expected" "$actual" "$input" >&2
    exit 1
  fi
}

assert_extracts_version '0.5.0-pre.8' '0.5.0-pre.8'
assert_extracts_version 'latest version: 0.5.0-pre.8' '0.5.0-pre.8'
assert_extracts_version 'Release 2026.05.14-alpha is available' '2026.05.14-alpha'
assert_extracts_version $'old 0.4.0-pre.7\nnew 0.5.0-pre.8' '0.5.0-pre.8'

printf 'hytale downloader version parsing ok\n'
