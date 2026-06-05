#!/usr/bin/env bash
# Reusable retry helper for network-bound CI steps.
#
# Usage:
#   source "${GITHUB_WORKSPACE}/.github/scripts/retry.sh"
#   retry <command> [args...]
#
# Retries the command up to RETRY_MAX_ATTEMPTS times (default 3) with
# exponential backoff starting at RETRY_INITIAL_DELAY seconds (default 5).
# Emits GitHub Actions ::warning:: annotations for each retried attempt and a
# final ::error:: annotation if every attempt fails, then returns the last exit
# code so the calling step still fails. Intended only for transient operations
# (downloads, registry/index fetches) — never wrap a deterministic scan in it.

retry() {
  local max_attempts="${RETRY_MAX_ATTEMPTS:-3}"
  local delay="${RETRY_INITIAL_DELAY:-5}"
  local attempt=1
  local status=0

  until "$@"; do
    status=$?
    if [ "${attempt}" -ge "${max_attempts}" ]; then
      echo "::error::command failed after ${attempt} attempts (exit ${status}): $*"
      return "${status}"
    fi
    echo "::warning::attempt ${attempt}/${max_attempts} failed (exit ${status}); retrying in ${delay}s: $*"
    sleep "${delay}"
    attempt=$((attempt + 1))
    delay=$((delay * 2))
  done
}
