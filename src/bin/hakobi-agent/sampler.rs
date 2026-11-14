use std::{
    sync::Arc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use hakobi::host_agent::{SystemDisk, SystemMetrics, SystemNetwork, SystemProcess};
use sysinfo::{Disks, Networks, System};
use tokio::sync::Mutex;

pub(super) struct SystemSampler {
    system: System,
    networks: Networks,
    last_sample: Instant,
    cached: SystemMetrics,
}

/// How often the background loop refreshes counters. Fixed 2s window gives
/// stable CPU/network deltas like btop, independent of request timing.
const SAMPLE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

impl SystemSampler {
    pub(super) fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_all();
        let mut networks = Networks::new_with_refreshed_list();
        // Warm the delta baseline with a short sleep so the first snapshot
        // has a real window instead of a near-zero one.
        std::thread::sleep(std::time::Duration::from_millis(200));
        system.refresh_all();
        networks.refresh(true);
        let mut sampler = Self {
            system,
            networks,
            last_sample: Instant::now(),
            cached: SystemMetrics::default(),
        };
        sampler.cached = sampler.measure();
        sampler
    }

    /// Measures counters over the window since the last measure call.
    fn measure(&mut self) -> SystemMetrics {
        let elapsed = self.last_sample.elapsed().as_secs_f64().max(0.001);
        self.system.refresh_all();
        self.networks.refresh(true);
        self.last_sample = Instant::now();
        let load = System::load_average();
        let cpus = self.system.cpus();
        let processes = self.processes();
        let disks = Self::disks();
        let networks = self.networks(elapsed);
        SystemMetrics {
            hostname: System::host_name().unwrap_or_default(),
            os_name: System::name().unwrap_or_default(),
            os_version: System::os_version().unwrap_or_default(),
            kernel_version: System::kernel_version().unwrap_or_default(),
            cpu_model: cpus
                .first()
                .map(|cpu| cpu.brand().to_owned())
                .unwrap_or_default(),
            cpu_count: cpus.len(),
            cpu_usage_percent: self.system.global_cpu_usage(),
            cpu_per_core: cpus.iter().map(|cpu| cpu.cpu_usage()).collect(),
            load_1: load.one,
            load_5: load.five,
            load_15: load.fifteen,
            uptime_seconds: System::uptime(),
            memory_total_bytes: self.system.total_memory(),
            memory_used_bytes: self.system.used_memory(),
            memory_available_bytes: self.system.available_memory(),
            swap_total_bytes: self.system.total_swap(),
            swap_used_bytes: self.system.used_swap(),
            process_count: self.system.processes().len(),
            processes,
            disks,
            networks,
            sampled_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_secs().to_string())
                .unwrap_or_default(),
        }
    }

    fn processes(&self) -> Vec<SystemProcess> {
        let mut processes = self
            .system
            .processes()
            .values()
            .map(|process| SystemProcess {
                pid: process.pid().as_u32(),
                user: process
                    .user_id()
                    .map(|user| user.to_string())
                    .unwrap_or_default(),
                cpu_percent: process.cpu_usage(),
                memory_percent: process.memory() as f32 / self.system.total_memory().max(1) as f32
                    * 100.0,
                command: process.name().to_string_lossy().into_owned(),
            })
            .collect::<Vec<_>>();
        processes.sort_by(|left, right| {
            right
                .cpu_percent
                .partial_cmp(&left.cpu_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        processes.truncate(20);
        processes
    }

    fn disks() -> Vec<SystemDisk> {
        Disks::new_with_refreshed_list()
            .list()
            .iter()
            .filter(|disk| disk.total_space() > 0)
            .map(|disk| SystemDisk {
                name: disk.name().to_string_lossy().into_owned(),
                mount: disk.mount_point().to_string_lossy().into_owned(),
                total_bytes: disk.total_space(),
                available_bytes: disk.available_space(),
            })
            .collect()
    }

    fn networks(&self, elapsed: f64) -> Vec<SystemNetwork> {
        self.networks
            .iter()
            .map(|(name, network)| SystemNetwork {
                name: name.clone(),
                received_bytes_per_second: (network.received() as f64 / elapsed) as u64,
                transmitted_bytes_per_second: (network.transmitted() as f64 / elapsed) as u64,
                total_received_bytes: network.total_received(),
                total_transmitted_bytes: network.total_transmitted(),
            })
            .collect()
    }

    /// Returns the most recent measurement; never blocks on counters.
    pub(super) fn snapshot(&self) -> SystemMetrics {
        self.cached.clone()
    }

    /// Background loop: keeps the cached snapshot current on a fixed window.
    pub(super) async fn run_refresh_loop(sampler: Arc<Mutex<Self>>) {
        loop {
            tokio::time::sleep(SAMPLE_INTERVAL).await;
            if let Ok(mut sampler) = sampler.try_lock() {
                sampler.cached = sampler.measure();
            }
        }
    }
}
