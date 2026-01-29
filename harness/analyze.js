const fs = require("fs");
const path = require("path");
const base = path.join(__dirname, "..", "results");

function loadTraces(dir) {
  const full = path.join(base, dir);
  if (!fs.existsSync(full)) return [];
  return fs
    .readdirSync(full)
    .filter((f) => f.endsWith("_trace.json"))
    .map((f) => JSON.parse(fs.readFileSync(path.join(full, f), "utf8")));
}

const baseline = loadTraces("baseline");
const hint = loadTraces("hint");
const hintFull = loadTraces("hint-full");

function stats(traces, label) {
  if (traces.length === 0) return;
  const avg = (arr, fn) => (arr.reduce((s, x) => s + fn(x), 0) / arr.length).toFixed(1);
  const avgI = (arr, fn) => Math.round(arr.reduce((s, x) => s + fn(x), 0) / arr.length);
  console.log("");
  console.log("=== " + label + " (" + traces.length + " runs) ===");
  console.log("  Avg tool calls:", avg(traces, (t) => t.tool_calls_count));
  console.log("  Avg files read:", avg(traces, (t) => (t.files_read || []).length));
  console.log("  Avg tokens (in+out):", avgI(traces, (t) => t.tokens_in + t.tokens_out));
  console.log("  Avg cost USD:", "$" + avg(traces, (t) => t.cost_usd));
  console.log("  Avg duration:", avg(traces, (t) => t.duration_seconds) + "s");
  console.log("  Discovered FMM:", traces.filter((t) => t.discovered_fmm).length + "/" + traces.length);
  console.log("  Used manifest:", traces.filter((t) => t.used_manifest).length + "/" + traces.length);
}

const byVariant = {};
for (const t of baseline) {
  if (!byVariant[t.variant]) byVariant[t.variant] = [];
  byVariant[t.variant].push(t);
}

for (const [v, traces] of Object.entries(byVariant).sort()) {
  stats(traces, "Baseline: " + v);
}
stats(hint, "Hint (1-line CLAUDE.md)");
stats(hintFull, "Full CLAUDE.md instructions");

// Accuracy check
const keyFiles = [
  "jwt.ts",
  "session.ts",
  "permissions.ts",
  "auth-middleware.ts",
  "auth-controller.ts",
  "auth-routes.ts",
  "hash.ts",
];

function checkAccuracy(traces, label) {
  console.log("\n--- Accuracy: " + label + " ---");
  for (let i = 0; i < traces.length; i++) {
    const answer = traces[i].final_answer || "";
    const found = keyFiles.filter((f) => answer.includes(f));
    const missed = keyFiles.filter((f) => !answer.includes(f));
    console.log(
      "  Run " + (i + 1) + ": " + found.length + "/" + keyFiles.length + " key files. Missed: " + (missed.join(", ") || "none")
    );
  }
}

for (const [v, traces] of Object.entries(byVariant).sort()) {
  checkAccuracy(traces, "Baseline: " + v);
}
checkAccuracy(hint, "Hint (1-line)");
checkAccuracy(hintFull, "Full CLAUDE.md");
