use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Metadata {
    pub git_commit: String,
    pub git_branch: String,
    pub dirty: bool,
    pub ledger_size: u64,
    pub runs: u64,
    pub warmup: u64,
    #[serde(default = "default_perf_frequency")]
    pub perf_frequency: u64,
    #[serde(default = "default_perf_call_graph")]
    pub perf_call_graph: String,
    #[serde(default = "default_enabled_tools")]
    pub enabled_tools: Vec<String>,
    pub command: String,
    pub binary: String,
    #[serde(default)]
    pub build_command: Option<String>,
    pub timestamp_unix_seconds: u64,
    pub tools: BTreeMap<String, String>,
}

fn default_perf_frequency() -> u64 {
    997
}

fn default_perf_call_graph() -> String {
    "dwarf".into()
}

fn default_enabled_tools() -> Vec<String> {
    vec!["hyperfine".into(), "perf".into(), "heaptrack".into()]
}

#[derive(Debug, Deserialize)]
pub struct HyperfineExport {
    pub results: Vec<HyperfineResult>,
}

#[derive(Debug, Deserialize)]
pub struct HyperfineResult {
    pub mean: f64,
    pub median: f64,
    pub exit_codes: Vec<i32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HeapSummary {
    pub allocations: u64,
    pub peak_heap_bytes: f64,
}
