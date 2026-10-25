//! Durations and process RSS, deliberately kept outside measured operations.
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Instant};

/// Raw per-operation latency samples in microseconds, preserving outliers.
#[derive(Default)]
pub struct Timings(BTreeMap<String, Vec<f64>>);
impl Timings {
    /// Times only the supplied operation; correctness checks belong after it.
    pub fn measure<T>(&mut self, name: &str, record: bool, operation: impl FnOnce() -> T) -> T {
        let start = Instant::now();
        let result = operation();
        let elapsed = start.elapsed().as_secs_f64() * 1e6;
        if record {
            self.push(name, elapsed);
        }
        result
    }
    /// Records an externally delimited sample in microseconds.
    pub fn push(&mut self, name: &str, us: f64) {
        self.0.entry(name.to_owned()).or_default().push(us);
    }
    /// Returns sample counts, nearest-rank p50/p95 and every raw sample.
    pub fn json(&self) -> Value {
        self.0
            .iter()
            .map(|(name, values)| {
                let mut sorted = values.clone();
                sorted.sort_by(f64::total_cmp);
                (
                    name.clone(),
                    json!({"samples": values.len(), "p50_us": percentile(&sorted, 50),
                "p95_us": percentile(&sorted, 95), "raw_us": values}),
                )
            })
            .collect::<serde_json::Map<_, _>>()
            .into()
    }
}
fn percentile(sorted: &[f64], percent: usize) -> f64 {
    sorted[(sorted.len() * percent).div_ceil(100) - 1]
}

/// Current resident bytes. Missing OS data is explicit null, never zero.
/// Invokes platform tooling only at checkpoints, outside latency timers.
pub fn resident_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let source = std::fs::read_to_string("/proc/self/status").ok()?;
        let line = source.lines().find(|line| line.starts_with("VmRSS:"))?;
        line.split_whitespace()
            .nth(1)?
            .parse::<u64>()
            .ok()?
            .checked_mul(1024)
    }
    #[cfg(target_os = "macos")]
    {
        let result = std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .ok()?;
        if !result.status.success() {
            return None;
        }
        String::from_utf8(result.stdout)
            .ok()?
            .trim()
            .parse::<u64>()
            .ok()?
            .checked_mul(1024)
    }
    #[cfg(target_os = "windows")]
    {
        let result = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-Command",
                &format!("(Get-Process -Id {}).WorkingSet64", std::process::id()),
            ])
            .output()
            .ok()?;
        if !result.status.success() {
            return None;
        }
        String::from_utf8(result.stdout).ok()?.trim().parse().ok()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

/// Records resident bytes alongside observable retained history depth.
pub fn checkpoint(name: &str, sessions: &[xiaomu_runtime::session::DocumentSession]) -> Value {
    json!({"stage": name, "rss_bytes": resident_bytes(),
        "history_depths": sessions.iter().map(|s| s.history_depths()).collect::<Vec<_>>()})
}
