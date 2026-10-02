#!/usr/bin/env bash
# Run the Stratify gate from the GitHub Action's inputs. action.yml passes
# each input as an environment variable, never as interpolated shell text,
# and this script quotes every value into an argument array.
set -euo pipefail

args=(check "${STRATIFY_PATH:-.}" --fail-on "${STRATIFY_FAIL_ON:-warning}" --format "${STRATIFY_FORMAT:-human}")
if [[ -n "${STRATIFY_BASELINE:-}" ]]; then
  args+=(--baseline "$STRATIFY_BASELINE")
fi
exec stratify "${args[@]}"
