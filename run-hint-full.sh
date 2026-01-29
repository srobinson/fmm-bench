#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
TASK="Find all files that export authentication-related functions. List each file path and the specific exports."
RESULTS_DIR="${SCRIPT_DIR}/results"
MODEL="${MODEL:-sonnet}"

mkdir -p "${RESULTS_DIR}/hint-full"

# Run manifest variant WITHOUT hiding user CLAUDE.md (full fmm knowledge)
for run in 1 2 3; do
  RUN_ID="$(date +%Y%m%d_%H%M%S)_manifest_hintfull"
  RAW_OUTPUT="${RESULTS_DIR}/${RUN_ID}_raw.jsonl"
  TRACE_OUTPUT="${RESULTS_DIR}/${RUN_ID}_trace.json"
  STDERR_LOG="${RESULTS_DIR}/${RUN_ID}_stderr.log"

  echo ">>> Full-hint run ${run}/3 (RUN_ID: ${RUN_ID})..."

  (
    cd "${SCRIPT_DIR}/repos/manifest"
    claude \
      --print \
      --output-format stream-json \
      --verbose \
      --model "$MODEL" \
      --no-session-persistence \
      --dangerously-skip-permissions \
      --disable-slash-commands \
      --tools "Read,Glob,Grep,Bash" \
      --max-budget-usd 2.00 \
      "$TASK" < /dev/null > "$RAW_OUTPUT" 2>"$STDERR_LOG" || true
  )

  node "${SCRIPT_DIR}/harness/parse-trace.js" \
    "$RAW_OUTPUT" "$TRACE_OUTPUT" "manifest" "$TASK" "0" "0"

  node -e "
    const t = require('${TRACE_OUTPUT}');
    console.log('  Tool calls:', t.tool_calls_count, '| Files:', (t.files_read||[]).length, '| FMM:', t.discovered_fmm, '| Manifest:', t.used_manifest);
  " 2>/dev/null || echo "  (parse failed)"
  echo ""
  sleep 2
done

# Move results
mv "${RESULTS_DIR}"/*_manifest_hintfull_* "${RESULTS_DIR}/hint-full/" 2>/dev/null || true
echo "Done. Results in results/hint-full/"
