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
    let MetricLabels {
        services_value,
        containers_value,
        services_foot,
        containers_foot,
        inventory_note,
        inventory_known,
        no_provider,
        all_failed,
    } = metric_labels(metrics);
    html! {
        section id="overview-metrics" class="mb-6" aria-label="System status" hx-get="/partials/metrics" hx-trigger="every 15s" hx-swap="outerHTML" {
            div class="mb-2 flex justify-end" {
                span class="text-[0.68rem] text-[#86868b]" data-updated { "Live" }
            }
            @if no_provider || all_failed {
                div class="mb-3 flex flex-col justify-between gap-2 rounded-xl bg-amber-50 px-4 py-3 text-sm text-amber-900 shadow-[0_1px_3px_rgba(0,0,0,0.04)] sm:flex-row sm:items-center" {
                    span { (inventory_note) ". Container and service metrics are unknown." }
                    a class="font-semibold text-amber-900 underline-offset-2 hover:underline" href="/providers" { "Connect a provider →" }
                }
            } @else if metrics.providers_failed > 0 {
                p class="mb-3 text-xs font-medium text-amber-700" { (inventory_note) ". Counts may be incomplete." }
            }
            div class="grid grid-cols-2 gap-4" {
                a class="relative block rounded-xl bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.04)] transition hover:shadow-md" href=(if inventory_known { "/stacks" } else { "/providers" }) {
                    div class="pr-10" {
                        div {
                            p class="m-0 text-[11px] font-semibold uppercase tracking-[0.08em] text-[#86868b]" { "Running services" }
                            p class="m-0 mt-2 text-[17px] font-semibold leading-tight tracking-[-0.04em] sm:text-[24px]" { (services_value) }
                        }
                        span class="absolute right-4 top-4 grid size-9 place-items-center rounded-lg bg-brand/10" aria-hidden="true" {
                            img src="/assets/art/3d/stack-compose.webp" alt="" class="size-8 object-contain";
                        }
                    }
                    p class="mb-0 mt-3 text-xs text-[#86868b]" { (services_foot) }
                }
                (metric_card("Active containers", &containers_value, &containers_foot, inventory_known, "/containers", "/providers", "/assets/art/3d/container-image.webp"))
                (environments_card(metrics, inventory_note.is_empty()))
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn metric_card(
    label: &str,
    value: &str,
    foot: &str,
    inventory_known: bool,
    href: &str,
    fallback_href: &str,
    art: &str,
) -> Markup {
    html! {
        a class="relative block rounded-xl bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.04)] transition hover:shadow-md" href=(if inventory_known { href } else { fallback_href }) {
            div class="pr-10" {
                div {
                    p class="m-0 text-[11px] font-semibold uppercase tracking-[0.08em] text-[#86868b]" { (label) }
                    p class="m-0 mt-2 text-[17px] font-semibold leading-tight tracking-[-0.04em] sm:text-[24px]" { (value) }
                }
                span class="absolute right-4 top-4 grid size-9 place-items-center rounded-lg bg-brand/10" aria-hidden="true" {
                    img src=(art) alt="" class="size-8 object-contain";
                }
            }
            p class="mb-0 mt-3 text-xs text-[#86868b]" { (foot) }
        }
    }
}

/// Overview summary card for CP5 environments. Counts stay unknown ("—")
/// until the catalog answers; failures surface instead of faking zero.
fn environments_card(metrics: &DashboardMetrics, inventory_known: bool) -> Markup {
    let value = if metrics.catalog_failed {
        "—"
    } else {
        "Open"
    };
    let foot = if metrics.catalog_failed {
        "Environment catalog unavailable".to_owned()
    } else if inventory_known {
        "Git or upload source on enrolled hosts".to_owned()
    } else {
        "Connect a provider to see live data".to_owned()
    };
    html! {
        a class="relative block rounded-xl bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.04)] transition hover:shadow-md" href="/environments" {
            div class="pr-10" {
                div {
                    p class="m-0 text-[11px] font-semibold uppercase tracking-[0.08em] text-[#86868b]" { "Environments" }
                    p class="m-0 mt-2 text-[17px] font-semibold leading-tight tracking-[-0.04em] sm:text-[24px]" { (value) }
                }
                span class="absolute right-4 top-4 grid size-9 place-items-center rounded-lg bg-brand/10" aria-hidden="true" {
                    i class="fa-solid fa-layer-group text-brand text-lg" {}
                }
            }
            p class="mb-0 mt-3 text-xs text-[#86868b]" { (foot) }
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

fn inventory_note(metrics: &DashboardMetrics, no_provider: bool, all_failed: bool) -> String {
    let plural = if metrics.providers_failed == 1 {
        ""
    } else {
        "s"
    };
    if metrics.catalog_failed {
        "Provider catalog unavailable".to_owned()
    } else if no_provider {
        "No Docker provider connected".to_owned()
    } else if all_failed {
        format!("{} provider{plural} unreachable", metrics.providers_failed)
    } else if metrics.providers_failed > 0 {
        format!(
            "Partial data · {} provider{plural} failed",
            metrics.providers_failed
        )
    } else {
        String::new()
    }
}

struct MetricLabels {
    services_value: String,
    containers_value: String,
    services_foot: String,
    containers_foot: String,
    inventory_note: String,
    inventory_known: bool,
    no_provider: bool,
    all_failed: bool,
}

fn metric_labels(metrics: &DashboardMetrics) -> MetricLabels {
    let inventory_known = metrics.providers_reachable > 0;
    let no_provider = metrics.providers_configured == 0;
    let all_failed = metrics.providers_configured > 0 && metrics.providers_reachable == 0;
    let inventory_note = inventory_note(metrics, no_provider, all_failed);
    let partial = if metrics.providers_failed > 0 {
        " · partial"
    } else {
        ""
    };
    let services_value = if inventory_known {
        format!("{} Running", metrics.services_running)
    } else {
        "—".to_owned()
    };
    let containers_value = containers_value(metrics, inventory_known);
    let services_foot = services_foot(metrics, inventory_known, &inventory_note, partial);
    let containers_foot = containers_foot(metrics, inventory_known, &inventory_note, partial);
    MetricLabels {
        services_value,
        containers_value,
        services_foot,
        containers_foot,
        inventory_note,
        inventory_known,
        no_provider,
        all_failed,
    }
}

fn containers_value(metrics: &DashboardMetrics, inventory_known: bool) -> String {
    if inventory_known {
        format!(
            "{} / {} Healthy",
            metrics.containers_running, metrics.containers_healthy
        )
    } else {
        "—".to_owned()
    }
}

fn services_foot(
    metrics: &DashboardMetrics,
    inventory_known: bool,
    inventory_note: &str,
    partial: &str,
) -> String {
    if inventory_known {
        format!(
            "{} {} · {} tracked{partial}",
            metrics.stacks,
            if metrics.stacks == 1 {
                "stack"
            } else {
                "stacks"
            },
            metrics.services_total
        )
    } else {
        inventory_note.to_owned()
    }
}

fn containers_foot(
    metrics: &DashboardMetrics,
    inventory_known: bool,
    inventory_note: &str,
    partial: &str,
) -> String {
    if inventory_known {
        format!(
            "{} restarting · {} total{partial}",
            metrics.containers_restarting, metrics.containers_total
        )
    } else {
        inventory_note.to_owned()
    }
}

/// View model for the dashboard page.
pub struct DashboardView<'a> {
    pub username: &'a str,
    pub metrics: &'a DashboardMetrics,
    pub notice: Option<&'a str>,
    pub agent_configured: bool,
    pub agent_online: bool,
    /// Aggregate CPU/memory points across all hosts for the 24h charts.
    pub history_points: &'a [hakobi::metrics::HistoryPoint],
    /// Most recent deployments across all environments.
    pub activity: &'a [ActivityRow<'a>],
}

/// One row of the recent-activity feed.
pub struct ActivityRow<'a> {
    pub deployment: &'a hakobi::catalog::Deployment,
    pub environment_name: &'a str,
}

/// Renders the dashboard: metric cards, 24h charts, recent activity.
pub fn dashboard(view: DashboardView<'_>) -> Markup {
    let DashboardView {
        username,
        metrics,
        notice,
        agent_configured,
        agent_online,
        history_points,
        activity,
    } = view;
    page(
        "Dashboard · Hakobi",
        html! {
            div class="min-h-screen bg-[#f5f5f7] lg:pl-64" {
                (sidebar("overview", "", username, ""))
                (mobile_header("", username))
                main class=(format!("{CONTENT_WIDTH} pb-24 pt-8 lg:pt-12")) {
                    (dashboard_alerts(agent_configured, agent_online, notice))
                    (metrics_section(metrics))
                    (dashboard_charts(history_points))
                    (recent_activity(activity))
                }
                footer class=(format!("{CONTENT_WIDTH} flex flex-col gap-4 py-6 text-[0.65rem] tracking-[0.1em] text-[#86868b] sm:flex-row sm:justify-between")) {
                    span { "Hakobi / 運び" }
                    span { "Your infra. Your data." }
                }
            }
        },
    )
}

/// 24h CPU and memory charts aggregated across all hosts.
fn dashboard_charts(points: &[hakobi::metrics::HistoryPoint]) -> Markup {
    let now = hakobi::metrics::now_ms();
    let from = now - 24 * 60 * 60_000;
    let cpu: Vec<(i64, f64)> = points
        .iter()
        .map(|p| (p.timestamp, p.cpu_percent))
        .collect();
    let memory: Vec<(i64, f64)> = points
        .iter()
        .map(|p| (p.timestamp, p.memory_percent))
        .collect();
    html! {
        section class="mt-8" aria-label="Server metrics" {
            h2 class="m-0 mb-3 text-[14px] font-semibold" { "Server metrics · 24h" }
            div class="grid grid-cols-1 gap-4 md:grid-cols-2" {
                div class="rounded-xl bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.04)]" {
                    (super::history::percent_chart(&cpu, from, now, "CPU"))
                }
                div class="rounded-xl bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.04)]" {
                    (super::history::percent_chart(&memory, from, now, "Memory"))
                }
            }
        }
    }
}

/// Recent deployments across all environments.
fn recent_activity(rows: &[ActivityRow<'_>]) -> Markup {
    html! {
        section class="mt-8" aria-label="Recent activity" {
            h2 class="m-0 mb-3 text-[14px] font-semibold" { "Recent activity" }
            @if rows.is_empty() {
                p class="m-0 rounded-xl bg-white p-4 text-sm text-muted shadow-[0_1px_3px_rgba(0,0,0,0.04)]" {
                    "No deployments yet. Create an environment to deploy."
                }
            } @else {
                div class="overflow-hidden rounded-[0.85rem] border border-line bg-white" {
                    @for row in rows {
                        div class="flex items-center justify-between gap-3 border-b border-line px-4 py-3 last:border-b-0" {
                            div class="min-w-0" {
                                p class="m-0 truncate text-[0.82rem] font-semibold" { (row.environment_name) }
                                p class="m-0 mt-0.5 text-xs text-[#86868b]" { (row.deployment.started_at) }
                            }
                            span class="text-xs font-medium" { (row.deployment.status) }
                        }
                    }
                }
            }
        }
    }
}
