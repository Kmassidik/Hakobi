//! Overview dashboard templates.

use maud::{html, Markup};

use super::readiness::readiness_section;
use hakobi::install::Readiness;

use super::{brand, page, sidebar, status_badge, CONTENT_WIDTH, EYEBROW};

pub struct DashboardView<'a> {
    pub base_url: &'a str,
    pub csrf_token: &'a str,
    pub tunnel_enabled: bool,
    pub username: &'a str,
    pub readiness: &'a Readiness,
    pub metrics: &'a DashboardMetrics,
    pub notice: Option<&'a str>,
    pub agent_configured: bool,
    pub agent_online: bool,
    pub install_running: bool,
}

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

fn instance_cards(base_url: &str, tunnel_enabled: bool) -> Markup {
    html! {
                        section class="mt-6 grid grid-cols-1 gap-4 md:grid-cols-3" aria-label="Instance details" {
                            article class="min-w-0 rounded-xl bg-white p-5 shadow-[0_1px_3px_rgba(0,0,0,0.04)]" {
                                p class=(format!("{EYEBROW} mb-3 flex items-center gap-2 text-[0.6rem] text-[#86868b]")) {
                                    i class="fa-solid fa-globe" aria-hidden="true" {}
                                    "Public address"
                                }
                                a class="block truncate text-[0.88rem] font-semibold no-underline" href=(base_url) { (base_url) }
                            }
                            article class="min-w-0 rounded-xl bg-white p-5 shadow-[0_1px_3px_rgba(0,0,0,0.04)]" {
                                p class=(format!("{EYEBROW} mb-3 flex items-center gap-2 text-[0.6rem] text-[#86868b]")) {
                                    i class="fa-solid fa-cloud" aria-hidden="true" {}
                                    "Delivery"
                                }
                                @if tunnel_enabled {
                                    p class="m-0 text-[0.88rem] font-semibold" {
                                        "Cloudflare Tunnel"
                                        span class="text-xs font-normal text-muted" { " · outbound only" }
                                    }
                                } @else {
                                    p class="m-0 text-[0.88rem] font-semibold" {
                                        "Loopback only"
                                        span class="text-xs font-normal text-muted" { " · local development" }
                                    }
                                }
                            }
                            article class="min-w-0 rounded-xl bg-white p-5 shadow-[0_1px_3px_rgba(0,0,0,0.04)]" {
                                p class=(format!("{EYEBROW} mb-3 flex items-center gap-2 text-[0.6rem] text-[#86868b]")) {
                                    i class="fa-solid fa-shield-halved" aria-hidden="true" {}
                                    "Session"
                                }
                                p class="m-0 text-[0.88rem] font-semibold" {
                                    "Authenticated"
                                    span class="text-xs font-normal text-muted" { " · 12 hour expiry" }
                                }
                            }
                        }
    }
}

fn hero_banner() -> Markup {
    html! {
                        section class="relative mb-6 flex min-h-[22rem] items-center overflow-hidden rounded-2xl bg-[#0f382c] px-8 py-10 text-white sm:px-12 lg:px-14 lg:py-14" aria-labelledby="hello-title" {
                            img src="/assets/art/hero-pipeline.webp" alt="" aria-hidden="true" class="hero-art pointer-events-none absolute inset-0 h-full w-full object-cover";
                            div aria-hidden="true" class="hero-scrim" {}
                            div class="relative z-10 max-w-[36rem]" {
                                p class="mb-3 text-[11px] font-semibold uppercase tracking-[0.16em] text-[#d8f078]" { "The first step is live" }
                                h2 id="hello-title" class="m-0 font-sans text-[clamp(2.6rem,5vw,4.6rem)] font-semibold leading-none tracking-[-0.05em]" {
                                    "Hello, world."
                                }
                                p class="mt-5 max-w-[31rem] text-[0.95rem] leading-[1.65] text-[#c3d2ca]" {
                                    "Your self-hosted control plane is running. Authentication and secure delivery are in place—ready for whatever you ship next."
                                }
                            }
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

fn greeting_row(username: &str, control_label: &str, control_healthy: bool) -> Markup {
    html! {
                        div class="mb-6 flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between" {
                            div {
                                p class="mb-2 text-[11px] font-semibold uppercase tracking-[0.14em] text-[#6e6e73]" { "Overview" }
                                h1 class="m-0 font-sans text-[clamp(1.9rem,3vw,2.6rem)] font-semibold tracking-[-0.045em]" {
                                    "Good to see you, " (username) "."
                                }
                            }
                            div class="flex items-center gap-3" {
                            (status_badge(control_label, control_healthy))
                        }
                        }
    }
}

pub fn dashboard(view: DashboardView<'_>) -> Markup {
    let DashboardView {
        base_url,
        csrf_token,
        tunnel_enabled,
        username,
        readiness,
        metrics,
        notice,
        agent_configured,
        agent_online,
        install_running,
    } = view;
    let control_healthy = !(agent_configured && !agent_online)
        && metrics.providers_failed == 0
        && !metrics.catalog_failed;
    let control_label = if agent_configured && !agent_online {
        "Host agent offline"
    } else if metrics.catalog_failed || metrics.providers_failed > 0 {
        "Control plane degraded"
    } else {
        "Control plane online"
    };
    page(
        "Dashboard · Hakobi",
        html! {
            div class="min-h-screen bg-[#f5f5f7] lg:pl-64" data-install-watch=(if install_running { "true" } else { "false" }) {
                (sidebar("overview", base_url, username, csrf_token))
                (mobile_header(csrf_token, username))

                main class=(format!("{CONTENT_WIDTH} pb-24 pt-8 lg:pt-12")) {
                    (dashboard_alerts(agent_configured, agent_online, notice))
                    (greeting_row(username, control_label, control_healthy))

                    (metrics_section(metrics))

                    (hero_banner())

                    (readiness_section(readiness, csrf_token, agent_online, agent_configured))

                    (instance_cards(base_url, tunnel_enabled))
                    (install_dialog())
                }

                footer class=(format!("{CONTENT_WIDTH} flex flex-col gap-4 py-6 text-[0.65rem] tracking-[0.1em] text-[#86868b] sm:flex-row sm:justify-between")) {
                    span { "Hakobi / 運び" }
                    span { "Your infra. Your data." }
                }
            }
        },
    )
}

fn install_dialog() -> Markup {
    html! {
        dialog id="install-tool" class="w-[90%] max-w-[44rem] rounded-[0.85rem] border border-line bg-surface p-6 text-ink" aria-labelledby="install-tool-title" {
            form method="dialog" class="m-0" {
                h2 id="install-tool-title" class="m-0 font-display text-[1.5rem] font-normal" { "Install tool" }
                p class="mt-3 text-sm leading-relaxed text-muted" {
                    "Run this command on the host. The dashboard never installs anything by itself."
                }
                code class="mt-4 block overflow-x-auto rounded-[0.6rem] border border-line bg-surface px-4 py-3 font-mono text-xs text-ink" data-install-command { "…" }
                div class="mt-5 flex flex-wrap gap-2" {
                    button type="button" class="rounded-lg bg-brand px-3.5 py-2 text-[0.78rem] font-semibold text-white" data-copy-command { "Copy command" }
                    button type="submit" class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-ink" data-dialog-close { "Close" }
                }
            }
        }
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
