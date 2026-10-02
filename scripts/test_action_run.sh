#!/usr/bin/env bash
# Check that action-run.sh passes Action inputs as literal arguments.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin"
cat >"$tmp/bin/stratify" <<'STUB'
#!/usr/bin/env bash
for a in "$@"; do printf '<%s>\n' "$a"; done
STUB
chmod +x "$tmp/bin/stratify"

out="$(PATH="$tmp/bin:$PATH" STRATIFY_PATH="dir with space" STRATIFY_FAIL_ON=warning \
  STRATIFY_FORMAT=human STRATIFY_BASELINE="base \$(touch $tmp/pwned).json" \
  bash "$here/action-run.sh")"
expected="<check>
<dir with space>
<--fail-on>
<warning>
<--format>
<human>
<--baseline>
<base \$(touch $tmp/pwned).json>"
if [[ "$out" != "$expected" ]]; then
  echo "unexpected arguments:"
  echo "$out"
  exit 1
fi
if [[ -e "$tmp/pwned" ]]; then
  echo "the baseline input was executed as shell code"
  exit 1
fi

out="$(PATH="$tmp/bin:$PATH" STRATIFY_PATH=. STRATIFY_FAIL_ON=never STRATIFY_FORMAT=json \
  STRATIFY_BASELINE= bash "$here/action-run.sh")"
if [[ "$out" == *"--baseline"* ]]; then
  echo "an empty baseline input must add no --baseline flag"
  exit 1
fi
echo "ok"
