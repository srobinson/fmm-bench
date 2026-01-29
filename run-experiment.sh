#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: run-experiment.sh [--hint] <variant> <task>"
  echo ""
  echo "Options:"
  echo "  --hint    Add CLAUDE.md hint about .fmm/ to the variant dir before running"
  echo ""
  echo "Arguments:"
  echo "  variant   One of: clean, inline, manifest"
  echo "  task      The task prompt to give Claude"
  echo ""
  echo "Environment:"
  echo "  MODEL     Claude model to use (default: sonnet)"
  exit 1
}

HINT=0
if [ "${1:-}" = "--hint" ]; then
  HINT=1
  shift
fi

VARIANT="${1:?$(usage)}"
TASK="${2:?$(usage)}"
MODEL="${MODEL:-sonnet}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_DIR="${SCRIPT_DIR}/repos/${VARIANT}"
RESULTS_DIR="${SCRIPT_DIR}/results"

HINT_SUFFIX=""
if [ "$HINT" = "1" ]; then
  HINT_SUFFIX="_hint"
fi
RUN_ID="$(date +%Y%m%d_%H%M%S)_${VARIANT}${HINT_SUFFIX}"

if [ ! -d "$REPO_DIR" ]; then
  echo "Error: Variant directory not found: ${REPO_DIR}" >&2
  echo "Available variants:" >&2
  ls "${SCRIPT_DIR}/repos/" >&2
  exit 1
fi

mkdir -p "$RESULTS_DIR"

RAW_OUTPUT="${RESULTS_DIR}/${RUN_ID}_raw.jsonl"
TRACE_OUTPUT="${RESULTS_DIR}/${RUN_ID}_trace.json"
STDERR_LOG="${RESULTS_DIR}/${RUN_ID}_stderr.log"

echo "=== Experiment Run ==="
echo "  Variant: ${VARIANT}${HINT_SUFFIX}"
echo "  Task:    ${TASK}"
echo "  Model:   ${MODEL}"
echo "  Run ID:  ${RUN_ID}"
echo "  Repo:    ${REPO_DIR}"
echo "  Hint:    ${HINT}"
echo ""

# Isolation: use --system-prompt to override any CLAUDE.md fmm knowledge.
# This gives a clean prompt with no fmm references while keeping auth.
SYSTEM_PROMPT="You are a helpful coding assistant. You have access to tools to explore a TypeScript codebase. Use them to answer the user's question accurately. Examine the project structure and source files as needed."

# If --hint, temporarily add a CLAUDE.md to the variant dir
if [ "$HINT" = "1" ]; then
  echo "Check .fmm/ for codebase index" > "${REPO_DIR}/CLAUDE.md"
  # Don't use --system-prompt when hint is active — let CLAUDE.md be discovered
  SYSTEM_PROMPT=""
fi

# Temporarily hide user-level CLAUDE.md to prevent fmm knowledge leakage
USER_CLAUDE_MD="${HOME}/.claude/CLAUDE.md"
HIDDEN_CLAUDE_MD="${HOME}/.claude/CLAUDE.md.experiment-hidden"
if [ -f "$USER_CLAUDE_MD" ]; then
  mv "$USER_CLAUDE_MD" "$HIDDEN_CLAUDE_MD"
  trap 'mv "$HIDDEN_CLAUDE_MD" "$USER_CLAUDE_MD" 2>/dev/null' EXIT
fi

START_TIME=$(date +%s)

# Build claude command
CLAUDE_ARGS=(
  --print
  --output-format stream-json
  --verbose
  --model "$MODEL"
  --no-session-persistence
  --dangerously-skip-permissions
  --disable-slash-commands
  --tools "Read,Glob,Grep,Bash"
  --max-budget-usd 2.00
)

if [ -n "$SYSTEM_PROMPT" ]; then
  CLAUDE_ARGS+=(--system-prompt "$SYSTEM_PROMPT")
fi

CLAUDE_ARGS+=("$TASK")

(
  cd "$REPO_DIR"
  claude "${CLAUDE_ARGS[@]}" < /dev/null > "$RAW_OUTPUT" 2>"$STDERR_LOG" || true
)

END_TIME=$(date +%s)
DURATION=$((END_TIME - START_TIME))

# Clean up hint file if we added one
if [ "$HINT" = "1" ]; then
  rm -f "${REPO_DIR}/CLAUDE.md"
fi

# Restore user CLAUDE.md
if [ -f "$HIDDEN_CLAUDE_MD" ]; then
  mv "$HIDDEN_CLAUDE_MD" "$USER_CLAUDE_MD"
  trap - EXIT
fi

echo "  Duration: ${DURATION}s"
echo "  Raw output: ${RAW_OUTPUT}"

# Parse the stream-json output into our trace format
node "${SCRIPT_DIR}/harness/parse-trace.js" \
  "$RAW_OUTPUT" \
  "$TRACE_OUTPUT" \
  "$VARIANT" \
  "$TASK" \
  "$DURATION" \
  "$HINT"

echo "  Trace: ${TRACE_OUTPUT}"
echo ""

# Quick summary
if [ -f "$TRACE_OUTPUT" ]; then
  echo "=== Quick Summary ==="
  node -e "
    const t = require('${TRACE_OUTPUT}');
    console.log('  Tool calls:      ', t.tool_calls_count);
    console.log('  Files read:      ', (t.files_read || []).length);
    console.log('  Lines read:      ', t.total_lines_read || 0);
    console.log('  Discovered fmm:  ', t.discovered_fmm);
    console.log('  Used manifest:   ', t.used_manifest);
    console.log('  Tokens in:       ', t.tokens_in);
    console.log('  Tokens out:      ', t.tokens_out);
    console.log('  Cost USD:        ', t.cost_usd);
  " 2>/dev/null || echo "  (summary parsing failed)"
fi
