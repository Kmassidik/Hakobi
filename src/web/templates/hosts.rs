//! Host list and live resource templates.

use maud::{html, Markup};

use hakobi::catalog::HostAgent;
use hakobi::providers::ContainerRow;

use super::{brand, format_bytes, page, sidebar, CONTENT_WIDTH, EYEBROW, FIELD};

fn host_uptime(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    if days > 0 {
        format!("{days}d {hours}h")
    } else {
        format!("{hours}h")
    }
}

fn host_percent(value: u64, total: u64) -> u8 {
    value
        .saturating_mul(100)
        .checked_div(total)
        .unwrap_or(0)
        .min(100) as u8
}

pub fn host_resources_live(
    system: &hakobi::host_agent::SystemMetrics,
    containers: &[ContainerRow],
    live_url: &str,
) -> Markup {
    let memory_percent = host_percent(system.memory_used_bytes, system.memory_total_bytes);
    let swap_percent = host_percent(system.swap_used_bytes, system.swap_total_bytes);
    let root_disk = system
        .disks
        .iter()
        .find(|disk| disk.mount == "/")
        .or_else(|| system.disks.first());
    let disk_percent = root_disk
        .map(|disk| host_percent(disk.total_bytes - disk.available_bytes, disk.total_bytes))
        .unwrap_or(0);
    let network_rx: u64 = system
        .networks
        .iter()
        .filter(|network| network.name != "lo")
        .map(|network| network.received_bytes_per_second)
        .sum();
    let network_tx: u64 = system
        .networks
        .iter()
        .filter(|network| network.name != "lo")
        .map(|network| network.transmitted_bytes_per_second)
        .sum();
    let network_name = system
        .networks
        .iter()
        .filter(|network| network.name != "lo")
        .max_by_key(|network| network.total_received_bytes)
        .map(|network| network.name.as_str())
        .unwrap_or("—");
    let cpu_usage = system.cpu_usage_percent;
    html! {
        div id="host-resources-live" hx-get=(live_url) hx-trigger="every 5s" hx-swap="outerHTML" {
            section class="mb-4 grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4" aria-label="Host totals" {
                a class="relative block rounded-xl bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.04)] transition hover:shadow-md" href="#cpu" {
                    div class="pr-10" {
                        div {
                            p class="m-0 text-[11px] font-semibold uppercase tracking-[0.08em] text-[#86868b]" { "CPU" }
                            p class="m-0 mt-2 text-[17px] font-semibold leading-tight tracking-[-0.04em] sm:text-[24px]" { (format!("{cpu_usage:.1}%")) }
                        }
                        span class="absolute right-4 top-4 grid size-9 place-items-center rounded-lg bg-brand/10" {
                            img src="/assets/art/3d/health.webp" alt="" class="size-8 object-contain";
                        }
                    }
                    p class="mb-0 mt-3 text-xs text-[#86868b]" {
                        "Load " (format!("{:.2} / {:.2} / {:.2}", system.load_1, system.load_5, system.load_15))
                    }
                }
                a class="relative block rounded-xl bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.04)] transition hover:shadow-md" href="#memory" {
                    div class="pr-10" {
                        div {
                            p class="m-0 text-[11px] font-semibold uppercase tracking-[0.08em] text-[#86868b]" { "Memory" }
                            p class="m-0 mt-2 text-[17px] font-semibold leading-tight tracking-[-0.04em] sm:text-[24px]" {
                                (format_bytes(system.memory_used_bytes)) " / " (format_bytes(system.memory_total_bytes))
                            }
                        }
                        span class="absolute right-4 top-4 grid size-9 place-items-center rounded-lg bg-brand/10" {
                            img src="/assets/art/3d/metrics.webp" alt="" class="size-8 object-contain";
                        }
                    }
                    progress class="metric-progress mt-3" value=(memory_percent) max="100" aria-label="Memory usage" {}
                    p class="mb-0 mt-2 text-[0.68rem] text-[#86868b]" { (memory_percent) "% used · swap " (format_bytes(system.swap_used_bytes)) }
                }
                a class="relative block rounded-xl bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.04)] transition hover:shadow-md" href="#disks" {
                    div class="pr-10" {
                        div {
                            p class="m-0 text-[11px] font-semibold uppercase tracking-[0.08em] text-[#86868b]" { "Disk" }
                            p class="m-0 mt-2 text-[17px] font-semibold leading-tight tracking-[-0.04em] sm:text-[24px]" {
                                @if let Some(disk) = root_disk {
                                    (format_bytes(disk.total_bytes - disk.available_bytes)) " / " (format_bytes(disk.total_bytes))
                                } @else { "—" }
                            }
                        }
                        span class="absolute right-4 top-4 grid size-9 place-items-center rounded-lg bg-brand/10" {
                            img src="/assets/art/3d/backup.webp" alt="" class="size-8 object-contain";
                        }
                    }
                    progress class="metric-progress mt-3" value=(disk_percent) max="100" aria-label="Disk usage" {}
                    p class="mb-0 mt-2 text-[0.68rem] text-[#86868b]" { (disk_percent) "% used · " (root_disk.map(|disk| disk.mount.as_str()).unwrap_or("no mounts")) }
                }
                a class="relative block rounded-xl bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.04)] transition hover:shadow-md" href="#network" {
                    div class="pr-10" {
                        div {
                            p class="m-0 text-[11px] font-semibold uppercase tracking-[0.08em] text-[#86868b]" { "Network" }
                            p class="m-0 mt-2 text-[17px] font-semibold leading-tight tracking-[-0.04em] sm:text-[24px]" {
                                (format_bytes(network_rx + network_tx)) "/s"
                            }
                        }
                        span class="absolute right-4 top-4 grid size-9 place-items-center rounded-lg bg-brand/10" {
                            img src="/assets/art/3d/tunnel.webp" alt="" class="size-8 object-contain";
                        }
                    }
                    p class="mb-0 mt-3 text-xs text-[#86868b]" { (network_name) " · RX " (format_bytes(network_rx)) "/s · TX " (format_bytes(network_tx)) "/s" }
                }
            }

            div class="mb-4 grid grid-cols-1 gap-4 xl:grid-cols-[1.45fr_0.9fr]" {
                section id="cpu" class="rounded-xl bg-white p-5 shadow-[0_1px_3px_rgba(0,0,0,0.04)]" aria-labelledby="host-cpu-title" {
                    div class="mb-4 flex items-start justify-between gap-4" {
                        div {
                            h2 id="host-cpu-title" class="m-0 text-[14px] font-semibold" { "CPU" }
                            p class="m-0 mt-1 text-xs text-[#86868b]" { (system.cpu_model) " · " (system.cpu_count) " cores" }
                        }
                        span class="text-[11px] text-[#86868b]" { "5s sample" }
                    }
                    div class="flex flex-wrap items-end justify-between gap-4" {
                        p class="m-0 text-[38px] font-semibold leading-none tracking-[-0.05em]" { (format!("{cpu_usage:.1}%")) }
                        p class="m-0 text-xs text-[#86868b]" { "load 1m " (format!("{:.2}", system.load_1)) " · 5m " (format!("{:.2}", system.load_5)) " · 15m " (format!("{:.2}", system.load_15)) }
                    }
                    div class="mt-5 grid grid-cols-8 gap-1.5" {
                        @for (index, usage) in system.cpu_per_core.iter().enumerate() {
                            div class="rounded bg-[#f5f5f7] p-1" title=(format!("CPU {index}: {usage:.1}%")) {
                                div class="mb-1 flex items-center justify-between gap-1 text-[7px] text-[#a1a1aa]" {
                                    span { (index) }
                                    span class="font-semibold text-[#68716c]" { (format!("{usage:.1}%")) }
                                }
                                progress class="metric-progress" value=(usage.min(100.0).round() as u8) max="100" aria-label=(format!("CPU {index}")) {}
                            }
                        }
                    }
                }
                section id="memory" class="rounded-xl bg-white p-5 shadow-[0_1px_3px_rgba(0,0,0,0.04)]" aria-labelledby="host-memory-title" {
                    div class="mb-4 flex items-start justify-between gap-4" {
                        div {
                            h2 id="host-memory-title" class="m-0 text-[14px] font-semibold" { "Memory" }
                            p class="m-0 mt-1 text-xs text-[#86868b]" { "physical + swap" }
                        }
                        span class="text-[11px] text-[#86868b]" { (memory_percent) "%" }
                    }
                    div class="grid gap-3 text-xs" {
                        div class="flex justify-between" { span class="text-[#86868b]" { "Used" } strong { (format_bytes(system.memory_used_bytes)) } }
                        div class="flex justify-between" { span class="text-[#86868b]" { "Available" } strong { (format_bytes(system.memory_available_bytes)) } }
                        div class="flex justify-between" { span class="text-[#86868b]" { "Swap" } strong { (format_bytes(system.swap_used_bytes)) " / " (format_bytes(system.swap_total_bytes)) } }
                    }
                    progress class="metric-progress mt-4" value=(memory_percent) max="100" aria-label="Memory usage" {}
                    progress class="metric-progress mt-2" value=(swap_percent) max="100" aria-label="Swap usage" {}
                    p class="mb-0 mt-4 text-[11px] text-[#86868b]" { "Uptime " (host_uptime(system.uptime_seconds)) " · " (system.os_name) " " (system.os_version) }
                }
            }

            div class="mb-4 grid grid-cols-1 gap-4 lg:grid-cols-2" {
                section id="network" class="rounded-xl bg-white p-5 shadow-[0_1px_3px_rgba(0,0,0,0.04)]" {
                    div class="mb-3 flex items-start justify-between gap-4" {
                        div { h2 class="m-0 text-[14px] font-semibold" { "Network" } p class="m-0 mt-1 text-xs text-[#86868b]" { "per-interface throughput" } }
                        span class="text-[11px] text-[#86868b]" { "RX " (format_bytes(network_rx)) "/s" }
                    }
                    table class="w-full border-collapse text-[11px]" {
                        thead { tr { th class="pb-2 text-left text-[9px] uppercase tracking-[0.07em] text-[#86868b]" { "Interface" } th class="pb-2 text-right text-[9px] uppercase tracking-[0.07em] text-[#86868b]" { "RX/s" } th class="pb-2 text-right text-[9px] uppercase tracking-[0.07em] text-[#86868b]" { "TX/s" } } }
                        tbody {
                            @for network in system.networks.iter().filter(|network| {
                                network.name != "lo"
                                    && network.total_received_bytes.saturating_add(network.total_transmitted_bytes) > 0
                            }).take(5) {
                                tr { td class="border-t border-[#f2f2f5] py-2 font-semibold" { (network.name) } td class="border-t border-[#f2f2f5] py-2 text-right" { (format_bytes(network.received_bytes_per_second)) "/s" } td class="border-t border-[#f2f2f5] py-2 text-right" { (format_bytes(network.transmitted_bytes_per_second)) "/s" } }
                            }
                        }
                    }
                }
                section id="disks" class="rounded-xl bg-white p-5 shadow-[0_1px_3px_rgba(0,0,0,0.04)]" {
                    div class="mb-3 flex items-start justify-between gap-4" {
                        div { h2 class="m-0 text-[14px] font-semibold" { "Storage" } p class="m-0 mt-1 text-xs text-[#86868b]" { "mounted filesystems" } }
                    }
                    table class="w-full border-collapse text-[11px]" {
                        thead { tr { th class="pb-2 text-left text-[9px] uppercase tracking-[0.07em] text-[#86868b]" { "Mount" } th class="pb-2 text-right text-[9px] uppercase tracking-[0.07em] text-[#86868b]" { "Used" } th class="pb-2 text-right text-[9px] uppercase tracking-[0.07em] text-[#86868b]" { "Free" } } }
                        tbody {
                            @for disk in system.disks.iter().take(4) {
                                tr { td class="border-t border-[#f2f2f5] py-2 font-semibold" { (disk.mount) } td class="border-t border-[#f2f2f5] py-2 text-right" { (format_bytes(disk.total_bytes - disk.available_bytes)) } td class="border-t border-[#f2f2f5] py-2 text-right" { (format_bytes(disk.available_bytes)) } }
                            }
                        }
                    }
                }
            }

            div class="grid grid-cols-1 gap-4 lg:grid-cols-2" {
                section class="rounded-xl bg-white p-5 shadow-[0_1px_3px_rgba(0,0,0,0.04)]" {
                    div class="mb-3 flex items-start justify-between gap-4" {
                        div { h2 class="m-0 text-[14px] font-semibold" { "Processes" } p class="m-0 mt-1 text-xs text-[#86868b]" { "top by CPU" } }
                        span class="text-[11px] text-[#86868b]" { (system.process_count) " total" }
                    }
                    table class="w-full border-collapse text-[11px]" {
                        thead { tr { th class="pb-2 text-left text-[9px] uppercase tracking-[0.07em] text-[#86868b]" { "PID / command" } th class="pb-2 text-right text-[9px] uppercase tracking-[0.07em] text-[#86868b]" { "CPU" } th class="pb-2 text-right text-[9px] uppercase tracking-[0.07em] text-[#86868b]" { "MEM" } } }
                        tbody {
                            @for process in system.processes.iter().take(10) {
                                tr {
                                    td class="border-t border-[#f2f2f5] py-2 font-semibold" { (process.pid) " · " (process.command) }
                                    td class="border-t border-[#f2f2f5] py-2 text-right" { (format!("{:.1}%", process.cpu_percent)) }
                                    td class="border-t border-[#f2f2f5] py-2 text-right" { (format!("{:.1}%", process.memory_percent)) }
                                }
                            }
                        }
                    }
                }
                section class="rounded-xl bg-white p-5 shadow-[0_1px_3px_rgba(0,0,0,0.04)]" {
                    div class="mb-3 flex items-start justify-between gap-4" {
                        div { h2 class="m-0 text-[14px] font-semibold" { "Containers" } p class="m-0 mt-1 text-xs text-[#86868b]" { "local Docker" } }
                        span class="text-[11px] text-[#86868b]" { (containers.len()) " total" }
                    }
                    table class="w-full border-collapse text-[11px]" {
                        thead { tr { th class="pb-2 text-left text-[9px] uppercase tracking-[0.07em] text-[#86868b]" { "Name" } th class="pb-2 text-right text-[9px] uppercase tracking-[0.07em] text-[#86868b]" { "State" } } }
                        tbody {
                            @for container in containers.iter().take(10) {
                                tr { td class="border-t border-[#f2f2f5] py-2 font-semibold" { (container.name) } td class="border-t border-[#f2f2f5] py-2 text-right" { (container.state) } }
                            }
                            @if containers.is_empty() {
                                tr { td class="border-t border-[#f2f2f5] py-2 text-[#86868b]" colspan="2" { "No containers." } }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn host_resources(
    base_url: &str,
    username: &str,
    host_label: &str,
    csrf_token: &str,
    system: &hakobi::host_agent::SystemMetrics,
    containers: &[ContainerRow],
    live_url: &str,
) -> Markup {
    page(
        format!("{host_label} resources · Hakobi").as_str(),
        html! {
            div class="min-h-screen lg:pl-64" {
                (sidebar("deployment", base_url, username, csrf_token))
                header class="lg:hidden border-b border-line bg-paper/95" {
                    div class=(format!("{CONTENT_WIDTH} flex h-[5.5rem] items-center justify-between gap-4")) {
                        (brand("Hakobi dashboard"))
                        div class="flex items-center gap-4" {
                            a class="text-[0.8rem] font-semibold text-muted no-underline hover:text-ink" href="/hosts" { "Deployment" }
                            span class="text-sm text-muted max-sm:hidden" { (username) }
                            form method="post" action="/logout" class="m-0" {
                                input type="hidden" name="csrf_token" value=(csrf_token);
                                button class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.8rem] font-semibold text-ink" type="submit" { "Sign out" }
                            }
                        }
                    }
                }
                main class=(format!("{CONTENT_WIDTH} pb-24 pt-8 lg:pt-12")) {
                    div class="mb-6 flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between" {
                        div {
                            p class="mb-2 text-[11px] font-semibold uppercase tracking-[0.14em] text-[#6e6e73]" { "Resources / " (host_label) }
                            h1 class="m-0 font-sans text-[clamp(1.9rem,3vw,2.6rem)] font-semibold tracking-[-0.045em]" { "Host resources." }
                            p class="mt-3 text-xs text-[#86868b]" {
                                (system.cpu_count) " vCPU · " (format_bytes(system.memory_total_bytes)) " RAM · " (system.kernel_version) " · up " (host_uptime(system.uptime_seconds))
                            }
                        }
                        div class="flex items-center gap-2" {
                            a class="rounded-full bg-white px-3.5 py-2 text-[0.75rem] font-semibold text-ink no-underline shadow-sm" href="/hosts" { "Deployment" }
                            span class="inline-flex items-center gap-2 rounded-full bg-white px-3.5 py-2 text-[0.75rem] font-semibold text-[#6e6e73] shadow-sm" {
                                span class="size-2 animate-pulse rounded-full bg-emerald-500" {}
                                "Live · 5s"
                            }
                        }
                    }
                    (host_resources_live(system, containers, live_url))
                }
            }
        },
    )
}

fn register_host_form(csrf_token: &str) -> Markup {
    html! {
                        section class="mb-8 rounded-[0.85rem] border border-line bg-surface/45 p-6" aria-labelledby="add-host-title" {
                            h2 id="add-host-title" class="mb-4 mt-0 font-display text-[1.6rem] font-normal" { "Register a remote agent" }
                            form method="post" action="/hosts" class="grid gap-4 md:grid-cols-3" {
                                input type="hidden" name="csrf_token" value=(csrf_token);
                                label class="flex flex-col gap-2 text-[0.8rem] font-bold" {
                                    "Name"
                                    input class=(FIELD) name="name" type="text" value="" maxlength="64" required;
                                }
                                label class="flex flex-col gap-2 text-[0.8rem] font-bold md:col-span-2" {
                                    "Agent URL"
                                    input class=(FIELD) name="agent_url" type="text" value="" placeholder="https://agent.example.com" maxlength="512" required;
                                }
                                label class="flex flex-col gap-2 text-[0.8rem] font-bold md:col-span-3" {
                                    "Bearer token"
                                    input class=(FIELD) name="token" type="password" autocomplete="off" maxlength="4096" required;
                                }
                                div class="md:col-span-3" {
                                    button class="rounded-[0.6rem] bg-brand px-4 py-3 font-bold text-white transition hover:bg-[#0f4130]" type="submit" {
                                        i class="fa-solid fa-plus mr-2" aria-hidden="true" {}
                                        "Register host"
                                    }
                                }
                            }
                            p class="mb-0 mt-4 text-xs leading-relaxed text-muted" {
                                "The token is encrypted at rest and never exposed in the dashboard."
                            }
                        }
    }
}

fn host_card(host: &HostAgent, csrf_token: &str) -> Markup {
    html! {
                                            article class="rounded-[0.85rem] border border-line bg-surface/45 p-6" {
                                                div class="flex flex-col justify-between gap-3 sm:flex-row sm:items-start" {
                                                    div {
                                                        h2 class="m-0 text-[1.1rem] font-bold" { (host.name) }
                                                        p class="mb-0 mt-1 break-all font-mono text-xs text-muted" { (host.agent_url) }
                                                        p class="m-0 mt-1 text-xs text-muted" { "Provider: " span class="font-semibold text-ink" { ("agent://".to_owned() + &host.id) } }
                                                    }
                                                    span class=(if host.enabled { "inline-flex w-fit items-center gap-2 rounded-full bg-brand-bright/10 px-3 py-1 text-xs font-semibold text-brand" } else { "inline-flex w-fit items-center gap-2 rounded-full bg-line/40 px-3 py-1 text-xs font-semibold text-muted" }) {
                                                        span class=(if host.enabled { "size-1.5 rounded-full bg-brand-bright" } else { "size-1.5 rounded-full bg-muted" }) aria-hidden="true" {}
                                                        (if host.enabled { "Enabled" } else { "Disabled" })
                                                    }
                                                }
                                                div class="mt-4 grid gap-2 text-xs text-muted sm:grid-cols-2" {
                                                    p class="m-0" {
                                                        "Last check: " span class="font-semibold text-ink" { (host.last_checked_at.as_deref().unwrap_or("never")) }
                                                    }
                                                    @if let Some(error) = &host.last_error {
                                                        p class="m-0 text-danger sm:col-span-2" { "Last error: " (error) }
                                                    }
                                                }
                                                div class="mt-5 flex flex-wrap gap-2" {
                                                    a class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-ink no-underline" href=(format!("/hosts/{}/resources", host.id)) { "Resources" }
                                                    form method="post" action=(format!("/hosts/{}/test", host.id)) class="m-0" {
                                                        input type="hidden" name="csrf_token" value=(csrf_token);
                                                        button class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold" type="submit" {
                                                            i class="fa-solid fa-plug mr-2" aria-hidden="true" {}
                                                            "Test"
                                                        }
                                                    }
                                                    form method="post" action=(format!("/hosts/{}/toggle", host.id)) class="m-0" {
                                                        input type="hidden" name="csrf_token" value=(csrf_token);
                                                        button class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold" type="submit" {
                                                            (if host.enabled { "Disable" } else { "Enable" })
                                                        }
                                                    }
                                                    form method="post" action=(format!("/hosts/{}/delete", host.id)) class="m-0" {
                                                        input type="hidden" name="csrf_token" value=(csrf_token);
                                                        button class="rounded-lg border border-danger/40 bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-danger" type="submit" { "Delete" }
                                                    }
                                                }
                                            }
    }
}

fn host_inventory_body(hosts: &[HostAgent], csrf_token: &str) -> Markup {
    html! {
                            @if hosts.is_empty() {
                                div class="rounded-[0.85rem] border border-dashed border-line p-8 text-center text-muted" {
                                    img src="/assets/art/illustrations/empty-providers.webp" alt="" aria-hidden="true" class="empty-art";
                                    p class="m-0" { "No hosts registered yet." }
                                }
                            } @else {
                                div class="grid gap-4" {
                                    @for host in hosts {
                                        (host_card(host, csrf_token))
                                    }
                                }
                            }
    }
}

fn host_inventory(hosts: &[HostAgent], csrf_token: &str) -> Markup {
    html! {
                (host_inventory_body(hosts, csrf_token))
    }
}

pub fn hosts(base_url: &str, username: &str, csrf_token: &str, hosts: &[HostAgent]) -> Markup {
    page(
        "Deployment · Hakobi",
        html! {
            div class="min-h-screen lg:pl-64" {
                (sidebar("deployment", base_url, username, csrf_token))
                header class="lg:hidden border-b border-line bg-paper/95" {
                    div class=(format!("{CONTENT_WIDTH} flex h-[5.5rem] items-center justify-between gap-4")) {
                        (brand("Hakobi dashboard"))
                        div class="flex items-center gap-4" {
                            a class="text-[0.8rem] font-semibold text-muted no-underline hover:text-ink" href="/" { "Back to overview" }
                            span class="text-sm text-muted max-sm:hidden" { (username) }
                            form method="post" action="/logout" class="m-0" {
                                input type="hidden" name="csrf_token" value=(csrf_token);
                                button class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.8rem] font-semibold text-ink" type="submit" { "Sign out" }
                            }
                        }
                    }
                }
                main class=(format!("{CONTENT_WIDTH} pb-20 pt-10 lg:pt-16")) {
                    div class="mb-8" {
                        p class=(format!("{EYEBROW} mb-3 text-brand-bright")) { "Deployment" }
                        h1 class="m-0 font-display text-[clamp(2.2rem,5vw,4rem)] font-normal tracking-[-0.04em]" { "Deployment" }
                        p class="mt-4 max-w-[44rem] leading-[1.7] text-muted" {
                            "Register remote agents that Hakobi can test, inspect, and operate. Each host gets its own provider row and encrypted credentials."
                        }
                    }
                    (register_host_form(csrf_token))
                    (host_inventory(hosts, csrf_token))
                    p class="mt-8 text-xs text-muted" { "Dashboard endpoint: " (base_url) }
                }
            }
        },
    )
}
