#!/usr/bin/env bash
set -euo pipefail

# ============================================================================
# FMM Issue Experiment
# Test FMM on a real GitHub issue against a real OSS repo.
# Usage: ./run-issue.sh <clean-repo> <fmm-repo> "<task-prompt>"
# ============================================================================

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CLEAN_SRC="${1:?Usage: $0 <clean-repo-dir> <fmm-repo-dir> '<task-prompt>'}"
FMM_SRC="${2:?Usage: $0 <clean-repo-dir> <fmm-repo-dir> '<task-prompt>'}"
TASK_PROMPT="${3:?Usage: $0 <clean-repo-dir> <fmm-repo-dir> '<task-prompt>'}"

RUN_DIR="$SCRIPT_DIR/runs/$(date +%Y%m%d-%H%M%S)"
MODEL="${MODEL:-sonnet}"
MAX_BUDGET="${MAX_BUDGET:-2.00}"
MAX_TURNS="${MAX_TURNS:-30}"

FMM_PREAMBLE='Files in this codebase contain FMM headers — structured metadata blocks at the top of each file listing exports, imports, and dependencies. Read these headers first to understand file purpose before reading full content. Use Grep to search FMM headers across files to find what you need.

'

log()  { echo -e "\033[0;34m[exp]\033[0m $*"; }
ok()   { echo -e "\033[0;32m[ok]\033[0m $*"; }
err()  { echo -e "\033[0;31m[err]\033[0m $*" >&2; }

command -v claude >/dev/null 2>&1 || { err "claude CLI not found"; exit 1; }
command -v jq    >/dev/null 2>&1 || { err "jq not found"; exit 1; }
[[ -d "$CLEAN_SRC" ]] || { err "Clean repo not found: $CLEAN_SRC"; exit 1; }
[[ -d "$FMM_SRC" ]]   || { err "FMM repo not found: $FMM_SRC"; exit 1; }

mkdir -p "$RUN_DIR"
log "Run dir:    $RUN_DIR"
log "Model:      $MODEL"
log "Budget:     \$$MAX_BUDGET per condition"
log "Max turns:  $MAX_TURNS"
log "Task:       ${TASK_PROMPT:0:80}..."

# ---- Copy repos ----
for variant in clean fmm; do
    DEST="$RUN_DIR/$variant"
    if [[ "$variant" == "clean" ]]; then
        cp -R "$CLEAN_SRC" "$DEST"
    else
        cp -R "$FMM_SRC" "$DEST"
    fi
    if [[ ! -d "$DEST/.git" ]]; then
        git -C "$DEST" init -q
        git -C "$DEST" add -A
        git -C "$DEST" commit -q -m "initial" --allow-empty
    fi
done
ok "Workspaces ready"

# ---- Run one condition ----
run_condition() {
    local variant="$1"
    local work_dir="$RUN_DIR/$variant"
    local out_jsonl="$RUN_DIR/${variant}-stream.jsonl"
    local prompt

    if [[ "$variant" == "fmm" ]]; then
        prompt="${FMM_PREAMBLE}${TASK_PROMPT}"
    else
        prompt="$TASK_PROMPT"
    fi

    log "Running \033[1m${variant}\033[0m condition..."
    local t0
    t0=$(date +%s)

    (cd "$work_dir" && claude \
        -p "$prompt" \
        --output-format stream-json \
        --verbose \
        --model "$MODEL" \
        --dangerously-skip-permissions \
        --tools "Read,Glob,Grep,Edit,Write,Bash" \
        --strict-mcp-config \
        --mcp-config '{"mcpServers":{}}' \
        --setting-sources "" \
        --disable-slash-commands \
        --no-session-persistence \
        --max-turns "$MAX_TURNS" \
        --max-budget-usd "$MAX_BUDGET" \
    ) > "$out_jsonl" 2>"$RUN_DIR/${variant}-stderr.log" || true

    local t1
    t1=$(date +%s)
    local wall=$(( t1 - t0 ))
    echo "$wall" > "$RUN_DIR/${variant}-wall.txt"
    ok "$variant done in ${wall}s ($(wc -l < "$out_jsonl" | tr -d ' ') stream lines)"
}

# ---- Extract metrics ----
extract_metrics() {
    local variant="$1"
    local jsonl="$RUN_DIR/${variant}-stream.jsonl"
    local out="$RUN_DIR/${variant}-metrics.json"

    if [[ ! -s "$jsonl" ]]; then
        echo '{"error":"no output"}' > "$out"
        return
    fi

    jq -s --arg wall "$(cat "$RUN_DIR/${variant}-wall.txt" 2>/dev/null || echo 0)" '
    [.[] | select(.type? == "assistant") | .message.content[]? | select(.type? == "tool_use")] as $tools |
    [$tools[] | .name] as $names |
    ($names | group_by(.) | map({key: .[0], value: length}) | from_entries) as $breakdown |
    ([$tools[] | select(.name == "Read") | .input.file_path // empty] | unique) as $files_read |
    ([.[] | select(.type? == "result")] | last // {}) as $result |
    ($result.usage // {}) as $usage |
    (($usage.input_tokens // 0) + ($usage.cache_read_input_tokens // 0) + ($usage.cache_creation_input_tokens // 0)) as $in_tokens |
    ($usage.output_tokens // 0) as $out_tokens |
    {
      total_tool_calls: ($names | length),
      tool_breakdown: $breakdown,
      files_read_count: ($files_read | length),
      files_read: $files_read,
      input_tokens: ($in_tokens / 1000 | round),
      output_tokens: ($out_tokens / 1000 | round),
      total_tokens: (($in_tokens + $out_tokens) / 1000 | round),
      cost_usd: ($result.total_cost_usd // 0),
      duration_ms: ($result.duration_ms // 0),
      num_turns: ($result.num_turns // 0),
      wall_seconds: ($wall | tonumber? // 0)
    }' "$jsonl" > "$out" 2>/dev/null || echo '{"error":"parse failed"}' > "$out"
}

# ---- Report ----
generate_report() {
    local clean="$RUN_DIR/clean-metrics.json"
    local fmm="$RUN_DIR/fmm-metrics.json"
    local report="$RUN_DIR/report.txt"

    jq -n --slurpfile c "$clean" --slurpfile f "$fmm" '
    ($c[0]) as $c | ($f[0]) as $f |
    def pct: if .[0] == 0 then "N/A" else ((.[1] - .[0]) / .[0] * 100 | round | tostring) + "%" end;
    {
      clean: $c,
      fmm: $f,
      delta: {
        tool_calls: ([$c.total_tool_calls, $f.total_tool_calls] | pct),
        files_read: ([$c.files_read_count, $f.files_read_count] | pct),
        total_tokens: ([$c.total_tokens, $f.total_tokens] | pct),
        cost: ([$c.cost_usd, $f.cost_usd] | pct),
        turns: ([$c.num_turns, $f.num_turns] | pct)
      }
    }' > "$RUN_DIR/results.json"

    echo "============================================================" > "$report"
    echo "  FMM ISSUE EXPERIMENT — RESULTS" >> "$report"
    echo "============================================================" >> "$report"
    echo "" >> "$report"
    echo "  Model:  $MODEL" >> "$report"
    echo "  Task:   ${TASK_PROMPT:0:100}..." >> "$report"
    echo "  Repo:   $(basename "$CLEAN_SRC")" >> "$report"
    echo "" >> "$report"

    jq -rn --slurpfile c "$clean" --slurpfile f "$fmm" '
    ($c[0]) as $c | ($f[0]) as $f |
    def delta: if .[0] == 0 then "  N/A" elif .[1] < .[0] then " " + (((.[1] - .[0]) / .[0] * 100 | round | tostring) + "%") else "+" + (((.[1] - .[0]) / .[0] * 100 | round | tostring) + "%") end;
    "  Metric                       Clean       FMM         Delta",
    "  -------------------------  ---------  ---------  ---------",
    "  Tool calls                 \($c.total_tool_calls | tostring | .[0:9] | . + " " * (9 - length))  \($f.total_tool_calls | tostring | .[0:9] | . + " " * (9 - length))  \([$c.total_tool_calls, $f.total_tool_calls] | delta)",
    "  Files read                 \($c.files_read_count | tostring | .[0:9] | . + " " * (9 - length))  \($f.files_read_count | tostring | .[0:9] | . + " " * (9 - length))  \([$c.files_read_count, $f.files_read_count] | delta)",
    "  Tokens (k)                 \($c.total_tokens | tostring | .[0:9] | . + " " * (9 - length))  \($f.total_tokens | tostring | .[0:9] | . + " " * (9 - length))  \([$c.total_tokens, $f.total_tokens] | delta)",
    "  Cost ($)                   \($c.cost_usd | tostring | .[0:9] | . + " " * (9 - length))  \($f.cost_usd | tostring | .[0:9] | . + " " * (9 - length))  \([$c.cost_usd, $f.cost_usd] | delta)",
    "  API turns                  \($c.num_turns | tostring | .[0:9] | . + " " * (9 - length))  \($f.num_turns | tostring | .[0:9] | . + " " * (9 - length))  \([$c.num_turns, $f.num_turns] | delta)",
    "",
    "  Files read (FMM):  \($f.files_read | join(", "))",
    "",
    "  Files read (Clean): \($c.files_read | join(", "))"
    ' >> "$report"

    cat "$report"
}

# ---- Main ----
log "Starting FMM issue experiment..."
echo ""

# Run FMM first — validate it works before spending on control
run_condition fmm
run_condition clean

log "Extracting metrics..."
extract_metrics clean
extract_metrics fmm

echo ""
generate_report

echo ""
log "Results:  $RUN_DIR/results.json"
log "Traces:   $RUN_DIR/{clean,fmm}-stream.jsonl"
log "Done."
