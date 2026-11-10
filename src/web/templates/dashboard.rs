//! Overview dashboard templates.

use maud::{html, Markup};

use super::{brand, page, sidebar, CONTENT_WIDTH};

#[derive(Debug, Clone, Default)]
pub struct DashboardMetrics {
    pub services_running: usize,
    pub services_total: usize,
    pub stacks: usize,
    pub containers_running: usize,
    pub containers_healthy: usize,
    pub containers_total: usize,
    pub containers_restarting: usize,
    pub providers_configured: usize,
    pub providers_reachable: usize,
    pub providers_failed: usize,
    pub catalog_failed: bool,
}

pub fn metrics_section(metrics: &DashboardMetrics) -> Markup {
    let stats = LiveStats::from_samples(&[], metrics);
    stat_cards(&stats, metrics)
}

struct LiveStats {
    cpu_percent: f64,
    memory_percent: f64,
    memory_used_gb: f64,
    memory_total_gb: f64,
    hosts_reporting: usize,
    fresh: bool,
}

impl LiveStats {
    fn from_samples(live: &[hakobi::metrics::LatestLive], _metrics: &DashboardMetrics) -> Self {
        if live.is_empty() {
            return Self {
                cpu_percent: 0.0,
                memory_percent: 0.0,
                memory_used_gb: 0.0,
                memory_total_gb: 0.0,
                hosts_reporting: 0,
                fresh: false,
            };
        }
        let sum = |f: &dyn Fn(&hakobi::metrics::LatestLive) -> f64| -> f64 {
            live.iter().map(f).sum::<f64>() / live.len() as f64
        };
        let sum_u64 = |f: &dyn Fn(&hakobi::metrics::LatestLive) -> u64| -> f64 {
            live.iter().map(f).sum::<u64>() as f64
        };
        let memory_used = sum_u64(&|s| s.memory_used_bytes);
        let memory_total = sum_u64(&|s| s.memory_total_bytes);
        Self {
            cpu_percent: sum(&|s| s.cpu_percent),
            memory_percent: if memory_total > 0.0 {
                memory_used / memory_total * 100.0
            } else {
                0.0
            },
            memory_used_gb: memory_used / 1024.0 / 1024.0 / 1024.0,
            memory_total_gb: memory_total / 1024.0 / 1024.0 / 1024.0,
            hosts_reporting: live.len(),
            fresh: true,
        }
    }
}

fn mobile_header(csrf_token: &str, username: &str) -> Markup {
    html! {
                    header class="bg-[#f5f5f7]/90 backdrop-blur lg:hidden" {
                        div class=(format!("{CONTENT_WIDTH} flex h-[5rem] items-center justify-between")) {
                            (brand("Hakobi dashboard"))
                            div class="flex items-center gap-3" {
                                span class="text-sm text-muted max-sm:hidden" aria-label={"Signed in as " (username)} { (username) }
                                form method="post" action="/logout" class="m-0" {
                                    input type="hidden" name="csrf_token" value=(csrf_token);
                                    button class="rounded-full bg-white px-3.5 py-2 text-[0.78rem] font-semibold text-ink shadow-sm" type="submit" {
                                        i class="fa-solid fa-right-from-bracket mr-2" aria-hidden="true" {}
                                        "Sign out"
                                    }
                                }
                            }
                        }
                    }
    }
}

fn notice_banner(notice: Option<&str>) -> Markup {
    html! {
                        @if let Some(message) = notice {
                            div class="mb-4 rounded-xl bg-white px-4 py-3 text-sm text-slate-700 shadow-[0_1px_3px_rgba(0,0,0,0.04)]" role="status" {
                                (message)
                            }
                        }
    }
}

fn dashboard_alerts(agent_configured: bool, agent_online: bool, notice: Option<&str>) -> Markup {
    html! {
                        @if agent_configured && !agent_online {
                            div class="mb-4 rounded-xl bg-amber-50 px-4 py-3 text-sm text-amber-900 shadow-[0_1px_3px_rgba(0,0,0,0.04)]" role="alert" {
                                strong { "Host agent unreachable." }
                                " Host installs are disabled until the agent is running."
                            }
                        }
                        (notice_banner(notice))
    }
}

/// Page header with the live chip and refresh controls.
fn dashboard_header(username: &str) -> Markup {
    html! {
        header class="mb-5 flex flex-wrap items-end justify-between gap-4" {
            div {
                p class="eyebrow m-0 mb-1.5" { "Dashboard" }
                h1 class="m-0 text-[26px] font-semibold leading-tight tracking-[-0.03em]" { "Good to see you, " (username) "." }
                p class="mt-1.5 text-xs text-[#68716c]" { "Live system health, resource utilization, and deployment activity" }
            }
            div class="flex flex-wrap items-center gap-2" {
                span class="inline-flex items-center gap-2 rounded-full border border-line bg-white px-3 py-1.5 text-[0.7rem] font-semibold text-[#57606a]" {
                    span class="size-1.5 animate-pulse rounded-full bg-emerald-500" aria-hidden="true" {}
                    "Live"
                }
                span class="rounded-full border border-line bg-white px-3 py-1.5 text-[0.7rem] font-semibold text-[#57606a]" { "Window · 24h" }
                span class="rounded-full border border-line bg-white px-3 py-1.5 text-[0.7rem] font-semibold text-[#57606a]" { "Refresh · 15s" }
            }
        }
    }
}

/// One stat card in the top row.
fn stat_card(
    label: &str,
    value: &str,
    foot: &str,
    delta: &str,
    delta_class: &str,
    icon: &str,
) -> Markup {
    html! {
        article class="card min-h-[6rem] rounded-xl p-4" {
            div class="flex items-center justify-between" {
                span class="text-[0.62rem] font-extrabold uppercase tracking-[0.09em] text-[#86868b]" { (label) }
                i class=(format!("fa-solid {icon} text-[#c7c7cc]")) aria-hidden="true" {}
            }
            p class="m-0 mt-2 font-mono text-[1.5rem] font-semibold leading-tight tracking-[-0.04em]" { (value) }
            div class="mt-1 flex items-center justify-between text-[0.68rem] text-[#86868b]" {
                span { (foot) }
                span class=(delta_class) { (delta) }
            }
        }
    }
}

/// The four top stat cards.
fn stat_cards(stats: &LiveStats, metrics: &DashboardMetrics) -> Markup {
    let cpu_class = if stats.cpu_percent > 80.0 {
        "delta bad"
    } else if stats.cpu_percent > 60.0 {
        "delta warn"
    } else {
        "delta good"
    };
    let mem_class = if stats.memory_percent > 85.0 {
        "delta bad"
    } else if stats.memory_percent > 70.0 {
        "delta warn"
    } else {
        "delta good"
    };
    let host_delta = if stats.fresh {
        "delta good"
    } else {
        "delta warn"
    };
    html! {
        div id="overview-metrics" class="mb-5 grid grid-cols-2 gap-3.5 xl:grid-cols-4" hx-get="/partials/metrics" hx-trigger="every 15s" hx-swap="outerHTML" aria-label="System status" {
            (stat_card(
                "CPU utilization",
                &format!("{:.2}%", stats.cpu_percent),
                &format!("{} hosts · avg live", stats.hosts_reporting),
                "live",
                cpu_class,
                "fa-gauge-high",
            ))
            (stat_card("Memory utilization", &format!("{:.1}%", stats.memory_percent), &format!("{:.1} / {:.1} GiB", stats.memory_used_gb, stats.memory_total_gb), "live", mem_class, "fa-memory"))
            (stat_card("Active containers", &format!("{} / {}", metrics.containers_healthy, metrics.containers_total.max(1)), &format!("{} restarting", metrics.containers_restarting), "provider trait", "delta good", "fa-cube"))
            (stat_card("Environments", &format!("{}", metrics.stacks), "registered", &format!("{} hosts up", stats.hosts_reporting), host_delta, "fa-layer-group"))
        }
    }
}

/// Section title row.
fn section_title(text: &str, small: &str) -> Markup {
    html! {
        div class="mb-2.5 mt-5 flex items-baseline justify-between" {
            h2 class="m-0 text-[13px] font-bold" { (text) }
            small class="font-mono text-[0.6rem] font-medium tracking-[0.06em] text-[#a1a1a6]" { (small) }
        }
    }
}

/// Time-series chart panels. The canvas elements are drawn by dashboard.js.
fn chart_section(points: &[hakobi::metrics::HistoryPoint]) -> Markup {
    let series: Vec<f64> = points.iter().map(|p| p.cpu_percent).collect();
    let memory: Vec<f64> = points.iter().map(|p| p.memory_percent).collect();
    let disk: Vec<f64> = points
        .iter()
        .map(|p| p.disk_used_bytes as f64 / 1024.0 / 1024.0 / 1024.0)
        .collect();
    let rx: Vec<f64> = points
        .iter()
        .map(|p| p.network_rx_bps * 8.0 / 1_000_000.0)
        .collect();
    let tx: Vec<f64> = points
        .iter()
        .map(|p| p.network_tx_bps * 8.0 / 1_000_000.0)
        .collect();
    html! {
        section {
            (section_title("Compute", "ALL HOSTS · AGGREGATED"))
            div class="grid grid-cols-1 gap-3.5 lg:grid-cols-2" {
                div class="card rounded-xl p-4" {
                    div class="mb-1 flex items-center justify-between" {
                        div {
                            p class="m-0 text-[13px] font-bold" { "CPU usage" }
                            p class="m-0 mt-0.5 font-mono text-[0.7rem] font-semibold text-[#3c3c43]" { (format!("{:.1}%", series.last().copied().unwrap_or(0.0))) }
                        }
                    }
                    canvas class="block h-[150px] w-full" data-chart="percent" data-series=(json_series(&[("Total", &series)])) aria-label="CPU history chart" role="img" {}
                }
                div class="card rounded-xl p-4" {
                    div class="mb-1 flex items-center justify-between" {
                        div {
                            p class="m-0 text-[13px] font-bold" { "Memory usage" }
                            p class="m-0 mt-0.5 font-mono text-[0.7rem] font-semibold text-[#3c3c43]" { (format!("{:.1}%", memory.last().copied().unwrap_or(0.0))) }
                        }
                    }
                    canvas class="block h-[150px] w-full" data-chart="percent" data-series=(json_series(&[("Used", &memory)])) aria-label="Memory history chart" role="img" {}
                }
            }
            (section_title("Storage & network", "TIME SERIES · AUTO SCALE"))
            div class="grid grid-cols-1 gap-3.5 lg:grid-cols-2" {
                div class="card rounded-xl p-4" {
                    div class="mb-1 flex items-center justify-between" {
                        div {
                            p class="m-0 text-[13px] font-bold" { "Disk usage" }
                            p class="m-0 mt-0.5 font-mono text-[0.7rem] font-semibold text-[#3c3c43]" { (format!("{:.2} GiB", disk.last().copied().unwrap_or(0.0))) }
                        }
                    }
                    canvas class="block h-[150px] w-full" data-chart="auto" data-series=(json_series(&[("Used", &disk)])) aria-label="Disk history chart" role="img" {}
                }
                div class="card rounded-xl p-4" {
                    div class="mb-1 flex items-center justify-between" {
                        div {
                            p class="m-0 text-[13px] font-bold" { "Network traffic" }
                            p class="m-0 mt-0.5 font-mono text-[0.7rem] font-semibold text-[#3c3c43]" { (format!("{:.1} Mbps", rx.last().copied().unwrap_or(0.0) + tx.last().copied().unwrap_or(0.0))) }
                        }
                    }
                    canvas class="block h-[150px] w-full" data-chart="auto" data-series=(json_series(&[("Receive", &rx), ("Transmit", &tx)])) aria-label="Network history chart" role="img" {}
                }
            }
        }
    }
}

/// Serializes chart series to JSON for the client-side renderer.
fn json_series(series: &[(&str, &[f64])]) -> String {
    let items: Vec<String> = series
        .iter()
        .map(|(name, values)| {
            format!(
                "{{\"name\":{},\"values\":{}}}",
                serde_json::to_string(name).unwrap_or_else(|_| "\"?\"".into()),
                serde_json::to_string(values).unwrap_or_else(|_| "[]".into())
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// View model for the dashboard page.
pub struct DashboardView<'a> {
    pub username: &'a str,
    pub metrics: &'a DashboardMetrics,
    pub notice: Option<&'a str>,
    pub agent_configured: bool,
    pub agent_online: bool,
    /// Aggregate points across all hosts for the time-series charts.
    pub history_points: &'a [hakobi::metrics::HistoryPoint],
    /// Newest live sample per host for the stat cards.
    pub live: &'a [hakobi::metrics::LatestLive],
    /// Most recent deployments across all environments.
    pub activity: &'a [ActivityRow<'a>],
}

/// One row of the recent-activity feed.
pub struct ActivityRow<'a> {
    pub deployment: &'a hakobi::catalog::Deployment,
    pub environment_name: &'a str,
}

/// Renders the dashboard: stat cards, time-series charts, runtime activity.
pub fn dashboard(view: DashboardView<'_>) -> Markup {
    let DashboardView {
        username,
        metrics,
        notice,
        agent_configured,
        agent_online,
        history_points,
        live,
        activity,
    } = view;
    let stats = LiveStats::from_samples(live, metrics);
    page(
        "Dashboard · Hakobi",
        html! {
            div class="min-h-screen bg-[#f5f5f7] lg:pl-60" {
                (sidebar("overview", "", username, ""))
                (mobile_header("", username))
                main class="mx-auto w-[92%] max-w-[96rem] pb-16 pt-8 lg:pt-9" {
                    (dashboard_alerts(agent_configured, agent_online, notice))
                    (dashboard_header(username))
                    (stat_cards(&stats, metrics))
                    (chart_section(history_points))
                    (runtime_activity(activity))
                    footer class="mt-5 flex flex-col gap-2 font-mono text-[0.68rem] text-[#a1a1a6] sm:flex-row sm:justify-between" {
                        span { "HAKOBI / 運び · ALL HOSTS" }
                        span { "Live · refreshes every 15s" }
                    }
                }
            }
        },
    )
}

/// Status chip class per deployment status.
fn status_tag(status: &str) -> Markup {
    let (class, label) = match status {
        "success" => ("tag", "success"),
        "building" | "running" => ("tag warn", status),
        "failed" => ("tag bad", "failed"),
        other => ("tag idle", other),
    };
    html! { span class=(class) { (label) } }
}

/// Recent deployments and recent control-plane events.
fn runtime_activity(rows: &[ActivityRow<'_>]) -> Markup {
    html! {
        section {
            (section_title("Runtime activity", "RECENT"))
            div class="grid grid-cols-1 gap-3.5 lg:grid-cols-[1.45fr_1fr]" {
                article class="overflow-hidden rounded-xl bg-white shadow-[0_1px_3px_rgba(0,0,0,0.04)]" {
                    div class="border-b border-line px-4 py-3" {
                        p class="m-0 text-[13px] font-bold" { "Recent deployments" }
                    }
                    @if rows.is_empty() {
                        p class="m-0 px-4 py-4 text-sm text-muted" { "No deployments yet. Create an environment to deploy." }
                    } @else {
                        div class="overflow-x-auto" {
                            table class="w-full whitespace-nowrap" {
                                thead {
                                    tr {
                                        th class="px-4 py-2.5 text-left text-[0.6rem] font-bold uppercase tracking-[0.08em] text-[#86868b]" { "Environment" }
                                        th class="px-4 py-2.5 text-left text-[0.6rem] font-bold uppercase tracking-[0.08em] text-[#86868b]" { "Version" }
                                        th class="px-4 py-2.5 text-left text-[0.6rem] font-bold uppercase tracking-[0.08em] text-[#86868b]" { "Status" }
                                        th class="px-4 py-2.5 text-left text-[0.6rem] font-bold uppercase tracking-[0.08em] text-[#86868b]" { "Trigger" }
                                    }
                                }
                                tbody {
                                    @for row in rows {
                                        tr class="border-t border-line" {
                                            td class="px-4 py-2.5 text-[0.8rem] font-semibold" { (row.environment_name) }
                                            td class="px-4 py-2.5 font-mono text-xs" { "v" (row.deployment.version) }
                                            td class="px-4 py-2.5" { (status_tag(&row.deployment.status)) }
                                            td class="px-4 py-2.5 text-[0.8rem] text-[#57606a]" { (row.deployment.trigger_type) }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                article class="overflow-hidden rounded-xl bg-white shadow-[0_1px_3px_rgba(0,0,0,0.04)]" {
                    div class="border-b border-line px-4 py-3" {
                        p class="m-0 text-[13px] font-bold" { "System events" }
                    }
                    (agent_events())
                }
            }
        }
    }
}

/// Recent control-plane events.
fn agent_events() -> Markup {
    html! {
        div class="font-mono text-[0.72rem]" {
            div class="grid grid-cols-[64px_48px_1fr] gap-2.5 border-b border-line px-4 py-2.5 last:border-b-0" {
                time class="text-[#a1a1a6]" { "now" }
                span class="font-bold text-[#3b82f6]" { "INFO" }
                span class="text-[#3c3c43]" { "Dashboard telemetry loaded" }
            }
        }
    }
}
