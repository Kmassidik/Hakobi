use serde::{Deserialize, Serialize};

/// Aggregate resource usage reported by `GET /v1/docker/metrics`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DockerMetrics {
    #[serde(default)]
    pub containers: usize,
    #[serde(default)]
    pub cpu_percent: f64,
    #[serde(default)]
    pub memory_used_bytes: u64,
    #[serde(default)]
    pub memory_limit_bytes: u64,
}

/// State of an asynchronous host installer job.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallJob {
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub output: String,
}

/// Current host resource usage.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SystemMetrics {
    #[serde(default)]
    pub hostname: String,
    #[serde(default)]
    pub os_name: String,
    #[serde(default)]
    pub os_version: String,
    #[serde(default)]
    pub kernel_version: String,
    #[serde(default)]
    pub cpu_model: String,
    #[serde(default)]
    pub cpu_count: usize,
    #[serde(default)]
    pub cpu_usage_percent: f32,
    #[serde(default)]
    pub cpu_per_core: Vec<f32>,
    #[serde(default)]
    pub load_1: f64,
    #[serde(default)]
    pub load_5: f64,
    #[serde(default)]
    pub load_15: f64,
    #[serde(default)]
    pub uptime_seconds: u64,
    #[serde(default)]
    pub memory_total_bytes: u64,
    #[serde(default)]
    pub memory_used_bytes: u64,
    #[serde(default)]
    pub memory_available_bytes: u64,
    #[serde(default)]
    pub swap_total_bytes: u64,
    #[serde(default)]
    pub swap_used_bytes: u64,
    #[serde(default)]
    pub process_count: usize,
    #[serde(default)]
    pub processes: Vec<SystemProcess>,
    #[serde(default)]
    pub disks: Vec<SystemDisk>,
    #[serde(default)]
    pub networks: Vec<SystemNetwork>,
    #[serde(default)]
    pub sampled_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SystemProcess {
    #[serde(default)]
    pub pid: u32,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub cpu_percent: f32,
    #[serde(default)]
    pub memory_percent: f32,
    #[serde(default)]
    pub command: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SystemDisk {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub mount: String,
    #[serde(default)]
    pub total_bytes: u64,
    #[serde(default)]
    pub available_bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SystemNetwork {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub received_bytes_per_second: u64,
    #[serde(default)]
    pub transmitted_bytes_per_second: u64,
    #[serde(default)]
    pub total_received_bytes: u64,
    #[serde(default)]
    pub total_transmitted_bytes: u64,
}
