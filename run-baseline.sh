#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
TASK="Find all files that export authentication-related functions. List each file path and the specific exports."
VARIANTS=("clean" "inline" "manifest")
RUNS_PER_VARIANT=3

echo "=== Baseline Experiment Batch ==="
echo "  Task: ${TASK}"
echo "  Variants: ${VARIANTS[*]}"
echo "  Runs per variant: ${RUNS_PER_VARIANT}"
echo ""

for variant in "${VARIANTS[@]}"; do
  for run in $(seq 1 "$RUNS_PER_VARIANT"); do
    echo ">>> Running ${variant} #${run}/${RUNS_PER_VARIANT}..."
    "${SCRIPT_DIR}/run-experiment.sh" "$variant" "$TASK" 2>&1
    echo ""
    # Small delay between runs to avoid rate limits
    sleep 2
  done
done

echo "=== All baseline runs complete ==="
echo ""

# Move results to baseline subdirectory
mkdir -p "${SCRIPT_DIR}/results/baseline"
mv "${SCRIPT_DIR}"/results/*_clean_* "${SCRIPT_DIR}/results/baseline/" 2>/dev/null || true
mv "${SCRIPT_DIR}"/results/*_inline_* "${SCRIPT_DIR}/results/baseline/" 2>/dev/null || true
mv "${SCRIPT_DIR}"/results/*_manifest_* "${SCRIPT_DIR}/results/baseline/" 2>/dev/null || true

echo "Results moved to results/baseline/"
echo ""

# Generate summary
echo "=== Baseline Summary ==="
node -e "
const fs = require('fs');
const dir = '${SCRIPT_DIR}/results/baseline';
const traces = fs.readdirSync(dir)
  .filter(f => f.endsWith('_trace.json'))
  .map(f => JSON.parse(fs.readFileSync(dir + '/' + f, 'utf8')));

const byVariant = {};
for (const t of traces) {
  if (!byVariant[t.variant]) byVariant[t.variant] = [];
  byVariant[t.variant].push(t);
}

console.log('Variant        | Runs | Avg Tools | Avg Files | Avg Tokens | Discovered FMM | Used Manifest');
console.log('---------------|------|-----------|-----------|------------|----------------|-------------');

for (const [v, runs] of Object.entries(byVariant).sort()) {
  const avgTools = (runs.reduce((s, r) => s + r.tool_calls_count, 0) / runs.length).toFixed(1);
  const avgFiles = (runs.reduce((s, r) => s + (r.files_read?.length || 0), 0) / runs.length).toFixed(1);
  const avgTokens = Math.round(runs.reduce((s, r) => s + r.tokens_in + r.tokens_out, 0) / runs.length);
  const discovered = runs.filter(r => r.discovered_fmm).length;
  const used = runs.filter(r => r.used_manifest).length;
  console.log(v.padEnd(15) + '| ' + String(runs.length).padEnd(5) + '| ' + avgTools.padEnd(10) + '| ' + avgFiles.padEnd(10) + '| ' + String(avgTokens).padEnd(11) + '| ' + (discovered + '/' + runs.length).padEnd(15) + '| ' + used + '/' + runs.length);
}
" 2>/dev/null || echo "(summary generation failed)"
