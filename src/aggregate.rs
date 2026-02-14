//! Aggregate metrics, statistical tests, and summary report generation.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::batch::CorpusEntry;
use crate::report::ComparisonReport;

/// Aggregated results from a batch run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateReport {
    /// Model used
    pub model: String,
    /// Runs per issue
    pub runs_per_issue: u32,
    /// Total issues attempted
    pub issues_total: usize,
    /// Issues completed successfully
    pub issues_completed: usize,
    /// Total cost
    pub total_cost: f64,
    /// Languages in corpus
    pub languages: Vec<String>,
    /// Overall metric comparison
    pub summary: MetricsSummary,
    /// Breakdown by language
    pub by_language: HashMap<String, MetricsSummary>,
    /// Breakdown by codebase size
    pub by_size: HashMap<String, MetricsSummary>,
    /// Per-issue results
    pub per_issue: Vec<IssueResult>,
}

/// Summary of paired metrics across runs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MetricsSummary {
    pub n: usize,
    pub tool_calls: PairedMetric,
    pub tokens: PairedMetric,
    pub cost: PairedMetric,
    pub duration: PairedMetric,
    pub read_calls: PairedMetric,
}

/// A paired metric (control vs fmm) with mean, delta, and optional p-value.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PairedMetric {
    pub control_mean: f64,
    pub fmm_mean: f64,
    pub delta_pct: f64,
    pub control_std: f64,
    pub fmm_std: f64,
    pub p_value: Option<f64>,
}

/// Result for a single issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueResult {
    pub id: String,
    pub language: String,
    pub size: String,
    pub control_tool_calls: f64,
    pub fmm_tool_calls: f64,
    pub control_cost: f64,
    pub fmm_cost: f64,
    pub control_grade: String,
    pub fmm_grade: String,
    pub delta_pct: f64,
}

impl AggregateReport {
    /// Build an aggregate report from individual comparison reports.
    pub fn from_reports(
        reports: Vec<(CorpusEntry, ComparisonReport)>,
        model: &str,
        runs_per_issue: u32,
    ) -> Self {
        let issues_total = reports.len();

        let mut all_pairs: Vec<MetricPair> = vec![];
        let mut by_lang: HashMap<String, Vec<MetricPair>> = HashMap::new();
        let mut by_size: HashMap<String, Vec<MetricPair>> = HashMap::new();
        let mut per_issue: Vec<IssueResult> = vec![];
        let mut total_cost = 0.0f64;
        let mut languages: Vec<String> = vec![];

        for (entry, report) in &reports {
            if !languages.contains(&entry.language) {
                languages.push(entry.language.clone());
            }

            for task in &report.task_results {
                let pair = MetricPair {
                    control_tools: task.control.tool_calls as f64,
                    fmm_tools: task.fmm.tool_calls as f64,
                    control_tokens: (task.control.input_tokens + task.control.output_tokens) as f64,
                    fmm_tokens: (task.fmm.input_tokens + task.fmm.output_tokens) as f64,
                    control_cost: task.control.total_cost_usd,
                    fmm_cost: task.fmm.total_cost_usd,
                    control_duration: task.control.duration_ms as f64,
                    fmm_duration: task.fmm.duration_ms as f64,
                    control_reads: task.control.read_calls as f64,
                    fmm_reads: task.fmm.read_calls as f64,
                };

                total_cost += pair.control_cost + pair.fmm_cost;

                all_pairs.push(pair.clone());
                by_lang
                    .entry(entry.language.clone())
                    .or_default()
                    .push(pair.clone());
                by_size
                    .entry(entry.size.clone())
                    .or_default()
                    .push(pair.clone());

                let control_grade = task
                    .control_eval
                    .as_ref()
                    .map(|e| e.grade.clone())
                    .unwrap_or_else(|| "-".to_string());
                let fmm_grade = task
                    .fmm_eval
                    .as_ref()
                    .map(|e| e.grade.clone())
                    .unwrap_or_else(|| "-".to_string());

                let delta = if pair.control_tools > 0.0 {
                    ((pair.control_tools - pair.fmm_tools) / pair.control_tools) * 100.0
                } else {
                    0.0
                };

                per_issue.push(IssueResult {
                    id: entry.id.clone(),
                    language: entry.language.clone(),
                    size: entry.size.clone(),
                    control_tool_calls: pair.control_tools,
                    fmm_tool_calls: pair.fmm_tools,
                    control_cost: pair.control_cost,
                    fmm_cost: pair.fmm_cost,
                    control_grade,
                    fmm_grade,
                    delta_pct: delta,
                });
            }
        }

        let summary = compute_summary(&all_pairs);
        let by_language: HashMap<String, MetricsSummary> = by_lang
            .into_iter()
            .map(|(k, v)| (k, compute_summary(&v)))
            .collect();
        let by_size_map: HashMap<String, MetricsSummary> = by_size
            .into_iter()
            .map(|(k, v)| (k, compute_summary(&v)))
            .collect();

        languages.sort();

        Self {
            model: model.to_string(),
            runs_per_issue,
            issues_total,
            issues_completed: reports.len(),
            total_cost,
            languages,
            summary,
            by_language,
            by_size: by_size_map,
            per_issue,
        }
    }

    /// Render as markdown.
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();

        md.push_str("# fmm A/B Benchmark Results\n\n");
        md.push_str(&format!(
            "**Corpus:** {} issues across {} languages\n",
            self.issues_total,
            self.languages.len()
        ));
        md.push_str(&format!(
            "**Model:** {} | **Runs per issue:** {}\n",
            self.model, self.runs_per_issue
        ));
        md.push_str(&format!("**Total cost:** ${:.2}\n\n", self.total_cost));

        // Summary table
        md.push_str("## Summary\n\n");
        md.push_str("| Metric | Control (avg) | FMM (avg) | Delta | p-value |\n");
        md.push_str("|--------|--------------|-----------|-------|---------|\n");
        format_metric_row(&mut md, "Tool calls", &self.summary.tool_calls, false);
        format_metric_row(&mut md, "Tokens (k)", &self.summary.tokens, true);
        format_metric_row(&mut md, "Cost ($)", &self.summary.cost, false);
        format_metric_row(&mut md, "Duration (ms)", &self.summary.duration, false);
        format_metric_row(&mut md, "Read calls", &self.summary.read_calls, false);
        md.push('\n');

        // By language
        if !self.by_language.is_empty() {
            md.push_str("## By Language\n\n");
            md.push_str("| Language | N | Ctrl Tools | FMM Tools | Delta |\n");
            md.push_str("|----------|---|-----------|-----------|-------|\n");
            let mut langs: Vec<_> = self.by_language.iter().collect();
            langs.sort_by_key(|(k, _)| (*k).clone());
            for (lang, s) in &langs {
                md.push_str(&format!(
                    "| {} | {} | {:.1} | {:.1} | {:.1}% |\n",
                    lang,
                    s.n,
                    s.tool_calls.control_mean,
                    s.tool_calls.fmm_mean,
                    s.tool_calls.delta_pct
                ));
            }
            md.push('\n');
        }

        // By size
        if !self.by_size.is_empty() {
            md.push_str("## By Codebase Size\n\n");
            md.push_str("| Size | N | Ctrl Tools | FMM Tools | Delta |\n");
            md.push_str("|------|---|-----------|-----------|-------|\n");
            for (size, s) in &self.by_size {
                md.push_str(&format!(
                    "| {} | {} | {:.1} | {:.1} | {:.1}% |\n",
                    size,
                    s.n,
                    s.tool_calls.control_mean,
                    s.tool_calls.fmm_mean,
                    s.tool_calls.delta_pct
                ));
            }
            md.push('\n');
        }

        // Per-issue results
        md.push_str("## Per-Issue Results\n\n");
        md.push_str(
            "| Issue | Language | Ctrl Tools | FMM Tools | Delta | Ctrl Grade | FMM Grade |\n",
        );
        md.push_str(
            "|-------|----------|-----------|-----------|-------|------------|----------|\n",
        );
        for r in &self.per_issue {
            md.push_str(&format!(
                "| {} | {} | {:.0} | {:.0} | {:.1}% | {} | {} |\n",
                r.id,
                r.language,
                r.control_tool_calls,
                r.fmm_tool_calls,
                r.delta_pct,
                r.control_grade,
                r.fmm_grade
            ));
        }

        md
    }
}

// ── internal ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct MetricPair {
    control_tools: f64,
    fmm_tools: f64,
    control_tokens: f64,
    fmm_tokens: f64,
    control_cost: f64,
    fmm_cost: f64,
    control_duration: f64,
    fmm_duration: f64,
    control_reads: f64,
    fmm_reads: f64,
}

fn compute_summary(pairs: &[MetricPair]) -> MetricsSummary {
    if pairs.is_empty() {
        return MetricsSummary::default();
    }

    let n = pairs.len();
    let ctrl_tools: Vec<f64> = pairs.iter().map(|p| p.control_tools).collect();
    let fmm_tools: Vec<f64> = pairs.iter().map(|p| p.fmm_tools).collect();
    let ctrl_tokens: Vec<f64> = pairs.iter().map(|p| p.control_tokens).collect();
    let fmm_tokens: Vec<f64> = pairs.iter().map(|p| p.fmm_tokens).collect();
    let ctrl_cost: Vec<f64> = pairs.iter().map(|p| p.control_cost).collect();
    let fmm_cost: Vec<f64> = pairs.iter().map(|p| p.fmm_cost).collect();
    let ctrl_dur: Vec<f64> = pairs.iter().map(|p| p.control_duration).collect();
    let fmm_dur: Vec<f64> = pairs.iter().map(|p| p.fmm_duration).collect();
    let ctrl_reads: Vec<f64> = pairs.iter().map(|p| p.control_reads).collect();
    let fmm_reads: Vec<f64> = pairs.iter().map(|p| p.fmm_reads).collect();

    MetricsSummary {
        n,
        tool_calls: paired_metric(&ctrl_tools, &fmm_tools),
        tokens: paired_metric(&ctrl_tokens, &fmm_tokens),
        cost: paired_metric(&ctrl_cost, &fmm_cost),
        duration: paired_metric(&ctrl_dur, &fmm_dur),
        read_calls: paired_metric(&ctrl_reads, &fmm_reads),
    }
}

fn paired_metric(control: &[f64], fmm: &[f64]) -> PairedMetric {
    let c_mean = mean(control);
    let f_mean = mean(fmm);
    let delta = if c_mean > 0.0 {
        ((c_mean - f_mean) / c_mean) * 100.0
    } else {
        0.0
    };

    let p_value = if control.len() >= 3 && fmm.len() >= 3 {
        Some(welch_t_test(control, fmm))
    } else {
        None
    };

    PairedMetric {
        control_mean: c_mean,
        fmm_mean: f_mean,
        delta_pct: delta,
        control_std: std_dev(control),
        fmm_std: std_dev(fmm),
        p_value,
    }
}

fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.iter().sum::<f64>() / xs.len() as f64
}

fn variance(xs: &[f64]) -> f64 {
    if xs.len() < 2 {
        return 0.0;
    }
    let m = mean(xs);
    let sum_sq: f64 = xs.iter().map(|x| (x - m).powi(2)).sum();
    sum_sq / (xs.len() as f64 - 1.0) // sample variance (Bessel's correction)
}

fn std_dev(xs: &[f64]) -> f64 {
    variance(xs).sqrt()
}

/// Two-sample Welch's t-test. Returns approximate p-value.
fn welch_t_test(a: &[f64], b: &[f64]) -> f64 {
    let n_a = a.len() as f64;
    let n_b = b.len() as f64;
    let var_a = variance(a);
    let var_b = variance(b);
    let mean_a = mean(a);
    let mean_b = mean(b);

    let se = (var_a / n_a + var_b / n_b).sqrt();
    if se < 1e-15 {
        return 1.0; // No variance — can't test
    }

    let t = (mean_a - mean_b) / se;

    // Welch-Satterthwaite degrees of freedom
    let num = (var_a / n_a + var_b / n_b).powi(2);
    let den = (var_a / n_a).powi(2) / (n_a - 1.0) + (var_b / n_b).powi(2) / (n_b - 1.0);
    let df = if den > 0.0 { num / den } else { 1.0 };

    // Approximate p-value using the t-distribution CDF approximation
    approx_t_pvalue(t.abs(), df)
}

/// Approximate two-tailed p-value for Student's t-distribution.
/// Uses the regularized incomplete beta function approximation.
fn approx_t_pvalue(t: f64, df: f64) -> f64 {
    // For large df, t approaches normal
    if df > 100.0 {
        // Normal approximation: 2 * Phi(-|t|)
        return 2.0 * normal_cdf(-t);
    }

    // Use the relationship: p = I_{df/(df+t^2)}(df/2, 1/2)
    // For simplicity, use a reasonable approximation
    let x = df / (df + t * t);
    2.0 * regularized_beta(x, df / 2.0, 0.5).min(1.0)
}

/// Approximate standard normal CDF using the Abramowitz & Stegun formula.
fn normal_cdf(x: f64) -> f64 {
    if x < -8.0 {
        return 0.0;
    }
    if x > 8.0 {
        return 1.0;
    }

    let t = 1.0 / (1.0 + 0.2316419 * x.abs());
    let d = 0.3989422804014327; // 1/sqrt(2*pi)
    let p = d * (-x * x / 2.0).exp();
    let c = t
        * (0.319381530
            + t * (-0.356563782 + t * (1.781477937 + t * (-1.821255978 + t * 1.330274429))));

    if x >= 0.0 {
        1.0 - p * c
    } else {
        p * c
    }
}

/// Regularized incomplete beta function I_x(a, b) via continued fraction.
fn regularized_beta(x: f64, a: f64, b: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }

    // Use the continued fraction representation
    let ln_beta = ln_gamma(a) + ln_gamma(b) - ln_gamma(a + b);
    let front = (a * x.ln() + b * (1.0 - x).ln() - ln_beta).exp() / a;

    // Lentz's continued fraction
    let mut c = 1.0f64;
    let mut d = 1.0 - (a + b) * x / (a + 1.0);
    if d.abs() < 1e-30 {
        d = 1e-30;
    }
    d = 1.0 / d;
    let mut f = d;

    for m in 1..200 {
        let m = m as f64;

        // Even step
        let an = m * (b - m) * x / ((a + 2.0 * m - 1.0) * (a + 2.0 * m));
        d = 1.0 + an * d;
        if d.abs() < 1e-30 {
            d = 1e-30;
        }
        c = 1.0 + an / c;
        if c.abs() < 1e-30 {
            c = 1e-30;
        }
        d = 1.0 / d;
        f *= d * c;

        // Odd step
        let an = -(a + m) * (a + b + m) * x / ((a + 2.0 * m) * (a + 2.0 * m + 1.0));
        d = 1.0 + an * d;
        if d.abs() < 1e-30 {
            d = 1e-30;
        }
        c = 1.0 + an / c;
        if c.abs() < 1e-30 {
            c = 1e-30;
        }
        d = 1.0 / d;
        let delta = d * c;
        f *= delta;

        if (delta - 1.0).abs() < 1e-10 {
            break;
        }
    }

    front * f
}

/// Stirling's approximation for ln(Gamma(x)).
fn ln_gamma(x: f64) -> f64 {
    // Lanczos approximation
    let coeffs = [
        76.18009172947146,
        -86.50532032941677,
        24.01409824083091,
        -1.231739572450155,
        0.1208650973866179e-2,
        -0.5395239384953e-5,
    ];

    let y = x;
    let tmp = x + 5.5;
    let tmp = tmp - (x + 0.5) * tmp.ln();
    let mut ser = 1.000000000190015f64;
    for (i, c) in coeffs.iter().enumerate() {
        ser += c / (y + 1.0 + i as f64);
    }
    -tmp + (2.5066282746310005 * ser / x).ln()
}

fn format_metric_row(md: &mut String, label: &str, m: &PairedMetric, divide_1k: bool) {
    let (ctrl, fmm) = if divide_1k {
        (m.control_mean / 1000.0, m.fmm_mean / 1000.0)
    } else {
        (m.control_mean, m.fmm_mean)
    };

    let p_str = match m.p_value {
        Some(p) if p < 0.001 => "<0.001".to_string(),
        Some(p) => format!("{:.3}", p),
        None => "-".to_string(),
    };

    md.push_str(&format!(
        "| {} | {:.1} | {:.1} | {:.1}% | {} |\n",
        label, ctrl, fmm, m.delta_pct, p_str
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mean() {
        assert!((mean(&[1.0, 2.0, 3.0]) - 2.0).abs() < 1e-10);
        assert_eq!(mean(&[]), 0.0);
    }

    #[test]
    fn test_std_dev() {
        let vals = [2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        let sd = std_dev(&vals);
        // Sample std dev with Bessel's correction: sqrt(32/7) ≈ 2.138
        assert!((sd - 2.138).abs() < 0.01);
    }

    #[test]
    fn test_variance_single_element() {
        assert_eq!(variance(&[5.0]), 0.0);
    }

    #[test]
    fn test_welch_t_test_identical() {
        let a = [10.0, 10.0, 10.0];
        let b = [10.0, 10.0, 10.0];
        let p = welch_t_test(&a, &b);
        assert!(
            p > 0.9,
            "p-value should be ~1.0 for identical samples: {}",
            p
        );
    }

    #[test]
    fn test_welch_t_test_different() {
        let control = [30.0, 35.0, 32.0, 28.0, 33.0];
        let fmm = [15.0, 18.0, 16.0, 14.0, 17.0];
        let p = welch_t_test(&control, &fmm);
        assert!(
            p < 0.01,
            "p-value should be small for clearly different samples: {}",
            p
        );
    }

    #[test]
    fn test_paired_metric() {
        let ctrl = [10.0, 12.0, 11.0];
        let fmm = [5.0, 6.0, 5.5];
        let m = paired_metric(&ctrl, &fmm);
        assert!((m.control_mean - 11.0).abs() < 0.01);
        assert!((m.fmm_mean - 5.5).abs() < 0.01);
        assert!(m.delta_pct > 45.0 && m.delta_pct < 55.0);
        assert!(m.p_value.is_some());
    }

    #[test]
    fn test_paired_metric_no_pvalue_small_n() {
        let ctrl = [10.0, 12.0];
        let fmm = [5.0, 6.0];
        let m = paired_metric(&ctrl, &fmm);
        assert!(m.p_value.is_none());
    }

    #[test]
    fn test_empty_aggregate() {
        let report = AggregateReport::from_reports(vec![], "sonnet", 1);
        assert_eq!(report.issues_total, 0);
        assert_eq!(report.summary.n, 0);
        let md = report.to_markdown();
        assert!(md.contains("fmm A/B Benchmark"));
    }

    #[test]
    fn test_normal_cdf_symmetry() {
        assert!((normal_cdf(0.0) - 0.5).abs() < 0.01);
        assert!((normal_cdf(0.0) + normal_cdf(0.0) - 1.0).abs() < 0.01);
        assert!(normal_cdf(3.0) > 0.99);
        assert!(normal_cdf(-3.0) < 0.01);
    }
}
