//! Container detail page with live stats and log sections.

use maud::{html, Markup, PreEscaped};

use super::{brand, page, sidebar, CONTENT_WIDTH, EYEBROW, FIELD};
use hakobi::providers::{DockerContainerUsage, DockerInspect, DockerLogs};

/// How the container detail page receives its data.
///
/// A missing container never reaches the template: the handler turns that into
/// a 404. A provider failure still renders the page shell with the bounded
/// error so the operator can navigate back.
pub struct ContainerDetailData {
    pub provider_id: String,
    pub provider_name: String,
    pub detail: DockerInspect,
}

/// Renders the container detail page.
///
/// Stats and logs poll their partial endpoints every two seconds; the initial
/// markup already contains the sections so the page is complete without
/// JavaScript.
#[allow(clippy::too_many_arguments)]
pub fn container_detail(
    base_url: &str,
    username: &str,
    csrf_token: &str,
    name: &str,
    detail: Option<&ContainerDetailData>,
    stats: Option<&DockerContainerUsage>,
    logs: Option<&DockerLogs>,
) -> Markup {
    page(
        "Container detail · Hakobi",
        html! {
            div class="min-h-screen lg:pl-64" {
                (sidebar("containers", base_url, username, csrf_token))
                header class="lg:hidden border-b border-line bg-paper/95" {
                    div class=(format!("{CONTENT_WIDTH} flex h-[5.5rem] items-center justify-between gap-4")) {
                        (brand("Hakobi dashboard"))
                        div class="flex items-center gap-4" {
                            a class="text-[0.8rem] font-semibold text-muted no-underline hover:text-ink" href="/containers" { "Containers" }
                            span class="text-sm text-muted max-sm:hidden" { (username) }
                            form method="post" action="/logout" class="m-0" {
                                input type="hidden" name="csrf_token" value=(csrf_token);
                                button class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.8rem] font-semibold text-ink" type="submit" { "Sign out" }
                            }
                        }
                    }
                }
                main class=(format!("{CONTENT_WIDTH} pb-20 pt-10 lg:pt-16")) {
                    a class="text-[0.75rem] font-semibold text-muted no-underline hover:text-ink" href="/containers" { "← All containers" }
                    @if let Some(data) = detail {
                        p class=(format!("{EYEBROW} mb-3 mt-6 text-brand-bright")) { (data.provider_name) }
                        h1 class="m-0 font-display text-[clamp(2rem,4.5vw,3.4rem)] font-normal tracking-[-0.04em] break-all" { (name) }
                        (detail_header_actions(&data.provider_id, name, csrf_token))
                        (detail_metadata(&data.detail))
                        (stats_slot(&data.provider_id, name, stats))
                        (logs_slot(&data.provider_id, name, logs))
                        (detail_delete_dialog(csrf_token, &data.provider_id, name))
                    } @else {
                        p class=(format!("{EYEBROW} mb-3 mt-6 text-brand-bright")) { "Docker" }
                        h1 class="m-0 font-display text-[clamp(2rem,4.5vw,3.4rem)] font-normal tracking-[-0.04em]" { (name) }
                        div class="mt-6 rounded-[0.85rem] border border-danger/30 bg-danger/5 p-5 text-sm text-danger" {
                            strong class="block" { "Provider read failed" }
                            span class="mt-1 block break-words" { (detail_error_text()) }
                        }
                    }
                }
            }
        },
    )
}

fn detail_error_text() -> &'static str {
    "The container could not be read from the provider. It may have been removed, or the provider is unreachable."
}

/// Action bar: lifecycle buttons plus the delete confirmation trigger.
fn detail_header_actions(provider_id: &str, name: &str, csrf_token: &str) -> Markup {
    let base = format!("/containers/{provider_id}/{name}");
    html! {
        div class="mt-5 flex flex-wrap items-center gap-2" {
            form method="post" action=(format!("{base}/start")) class="m-0" {
                input type="hidden" name="csrf_token" value=(csrf_token);
                button class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-ink" type="submit" { "Start" }
            }
            form method="post" action=(format!("{base}/stop")) class="m-0" {
                input type="hidden" name="csrf_token" value=(csrf_token);
                button class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-ink" type="submit" { "Stop" }
            }
            form method="post" action=(format!("{base}/restart")) class="m-0" {
                input type="hidden" name="csrf_token" value=(csrf_token);
                button class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-ink" type="submit" { "Restart" }
            }
            button type="button" class="rounded-lg border border-danger/40 bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-danger" data-dialog-open="delete-container" data-delete-action=(format!("{base}/delete")) data-delete-name=(name) {
                "Delete…"
            }
        }
    }
}

/// Metadata table: image, state, health, command, ports, mounts, env, labels.
fn detail_metadata(detail: &DockerInspect) -> Markup {
    html! {
        section class="mt-6 rounded-[0.85rem] border border-line bg-surface/45 p-6" aria-label="Container metadata" {
            dl class="m-0 grid grid-cols-1 gap-x-10 gap-y-4 md:grid-cols-2" {
                (detail_row("Image", detail.image.as_str()))
                (detail_row("State", detail.state.as_str()))
                (detail_row("Health", detail.health.as_deref().unwrap_or("none")))
                (detail_row("Command", detail.command.as_str()))
                (detail_row("Created", detail.created_at.as_str()))
                (detail_row("Started", detail.started_at.as_str()))
                (detail_row("Restarts", detail.restart_count.to_string().as_str()))
                (detail_row("Platform", detail.platform.as_str()))
                (detail_row("Image ID", detail.image_id.as_str()))
                (detail_row("Container ID", detail.id.as_str()))
            }
            (detail_ports(detail))
            (detail_mounts(detail))
            (detail_env(detail))
            (detail_labels(detail))
        }
    }
}

fn detail_row(label: &str, value: &str) -> Markup {
    let display = if value.is_empty() { "—" } else { value };
    html! {
        div class="min-w-0" {
            dt class="text-[0.65rem] uppercase tracking-[0.12em] text-muted" { (label) }
            dd class="m-0 mt-1 break-words font-mono text-[0.8rem] text-ink" { (display) }
        }
    }
}

fn detail_ports(detail: &DockerInspect) -> Markup {
    html! {
        div class="mt-5 border-t border-line pt-4" {
            h3 class="m-0 text-[0.68rem] uppercase tracking-[0.12em] text-muted" { "Ports" }
            @if detail.ports.is_empty() {
                p class="mt-2 text-sm text-muted" { "No published ports." }
            } @else {
                ul class="mt-2 flex flex-wrap gap-2" {
                    @for port in &detail.ports {
                        li class="rounded-md border border-line bg-surface px-2.5 py-1 font-mono text-[0.72rem] text-ink" {
                            @if let Some(host_port) = port.host_port {
                                (port.host_ip.as_deref().unwrap_or("0.0.0.0")) ":" (host_port) " → "
                            }
                            (port.container_port) "/" (port.protocol)
                        }
                    }
                }
            }
        }
    }
}

fn detail_mounts(detail: &DockerInspect) -> Markup {
    html! {
        div class="mt-5 border-t border-line pt-4" {
            h3 class="m-0 text-[0.68rem] uppercase tracking-[0.12em] text-muted" { "Mounts" }
            @if detail.mounts.is_empty() {
                p class="mt-2 text-sm text-muted" { "No mounts." }
            } @else {
                ul class="mt-2 flex flex-col gap-1.5" {
                    @for mount in &detail.mounts {
                        li class="break-all font-mono text-[0.72rem] text-ink" {
                            (mount.source) " → " (mount.destination)
                            @if mount.read_only {
                                span class="ml-2 rounded-md bg-surface px-1.5 py-0.5 text-[0.62rem] text-muted" { "read-only" }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn detail_env(detail: &DockerInspect) -> Markup {
    html! {
        div class="mt-5 border-t border-line pt-4" {
            h3 class="m-0 text-[0.68rem] uppercase tracking-[0.12em] text-muted" { "Environment" }
            @if detail.env.is_empty() {
                p class="mt-2 text-sm text-muted" { "No environment variables." }
            } @else {
                ul class="mt-2 flex flex-col gap-1.5" {
                    @for variable in &detail.env {
                        li class="break-all font-mono text-[0.72rem] text-ink" { (variable) }
                    }
                }
            }
        }
    }
}

fn detail_labels(detail: &DockerInspect) -> Markup {
    html! {
        div class="mt-5 border-t border-line pt-4" {
            h3 class="m-0 text-[0.68rem] uppercase tracking-[0.12em] text-muted" { "Labels" }
            @if detail.labels.is_empty() {
                p class="mt-2 text-sm text-muted" { "No labels." }
            } @else {
                ul class="mt-2 flex flex-wrap gap-2" {
                    @for (key, value) in &detail.labels {
                        li class="rounded-md border border-line bg-surface px-2.5 py-1 font-mono text-[0.72rem] text-ink" {
                            (key) "=" (value)
                        }
                    }
                }
            }
        }
    }
}

/// Stats section wrapped in the HTMX poll slot.
fn stats_slot(provider_id: &str, name: &str, stats: Option<&DockerContainerUsage>) -> Markup {
    let url = format!("/containers/{provider_id}/{name}/stats");
    html! {
        section id="container-stats" class="mt-6" aria-label="Live resource usage"
            hx-get=(url) hx-trigger="every 2s" hx-swap="outerHTML" {
            (container_stats_section(name, &stats_or_placeholder(stats)))
        }
    }
}

fn stats_or_placeholder(stats: Option<&DockerContainerUsage>) -> DockerContainerUsage {
    stats.cloned().unwrap_or_default()
}

/// The swappable stats fragment: same markup on page load and on every poll.
pub fn container_stats_section(name: &str, stats: &DockerContainerUsage) -> Markup {
    let memory_percent = if stats.memory_limit_bytes > 0 {
        (stats.memory_used_bytes as f64 / stats.memory_limit_bytes as f64) * 100.0
    } else {
        0.0
    };
    html! {
        div class="rounded-[0.85rem] border border-line bg-surface/45 p-6" {
            div class="flex items-center justify-between gap-3" {
                h2 class="m-0 text-[1.05rem] font-bold" { "Resource usage" }
                span class="text-[0.65rem] text-muted" { (format!("live · {name}")) }
            }
            dl class="m-0 mt-4 grid grid-cols-2 gap-4 md:grid-cols-4" {
                (stat_card("CPU", format!("{:.1} %", stats.cpu_percent).as_str()))
                (stat_card("Memory", format!("{:.1} MiB", mib_value(stats.memory_used_bytes)).as_str()))
                (stat_card("Memory limit", format!("{:.1} MiB", mib_value(stats.memory_limit_bytes)).as_str()))
                (stat_card("Memory used", format!("{:.1} %", memory_percent).as_str()))
                (stat_card("Network received", format!("{:.1} KiB", kib_value(stats.network_rx_bytes)).as_str()))
                (stat_card("Network sent", format!("{:.1} KiB", kib_value(stats.network_tx_bytes)).as_str()))
            }
        }
    }
}

fn stat_card(label: &str, value: &str) -> Markup {
    html! {
        div class="rounded-[0.6rem] border border-line bg-surface px-4 py-3" {
            dt class="text-[0.62rem] uppercase tracking-[0.12em] text-muted" { (label) }
            dd class="m-0 mt-1 font-mono text-[0.95rem] text-ink" { (value) }
        }
    }
}

/// Rendered when the live stats poll fails; swapped in by HTMX on the error.
pub fn container_stats_error(message: &str) -> Markup {
    html! {
        div class="rounded-[0.85rem] border border-danger/30 bg-danger/5 p-5 text-sm text-danger" {
            strong class="block" { "Stats unavailable" }
            span class="mt-1 block break-words" { (message) }
        }
    }
}

/// Logs section wrapped in the HTMX poll slot.
fn logs_slot(provider_id: &str, name: &str, logs: Option<&DockerLogs>) -> Markup {
    let url = format!("/containers/{provider_id}/{name}/logs");
    html! {
        section id="container-logs" class="mt-6" aria-label="Container logs"
            hx-get=(url) hx-trigger="every 2s" hx-swap="outerHTML" {
            (container_logs_section(name, &logs.cloned().unwrap_or_default()))
        }
    }
}

/// The swappable log fragment: preformatted tail with a truncation marker.
pub fn container_logs_section(name: &str, logs: &DockerLogs) -> Markup {
    html! {
        div class="rounded-[0.85rem] border border-line bg-surface/45 p-6" {
            div class="flex items-center justify-between gap-3" {
                h2 class="m-0 text-[1.05rem] font-bold" { "Logs" }
                span class="text-[0.65rem] text-muted" { (format!("tail · {} lines · {}", logs.lines.len(), name)) }
            }
            @if logs.truncated {
                p class="mb-0 mt-2 text-[0.68rem] text-muted" { "Showing the last available lines; older output was cut off." }
            }
            pre class="mt-3 max-h-[28rem] overflow-auto rounded-[0.6rem] border border-line bg-surface p-4 font-mono text-[0.72rem] leading-relaxed text-ink" {
                @if logs.lines.is_empty() {
                    "No log output."
                } @else {
                    (PreEscaped(escape_log_lines(&logs.lines)))
                }
            }
        }
    }
}

/// Rendered when the live log poll fails.
pub fn container_logs_error(message: &str) -> Markup {
    html! {
        div class="rounded-[0.85rem] border border-danger/30 bg-danger/5 p-5 text-sm text-danger" {
            strong class="block" { "Logs unavailable" }
            span class="mt-1 block break-words" { (message) }
        }
    }
}

/// Escapes each line for safe `<pre>` rendering and joins them.
fn escape_log_lines(lines: &[String]) -> String {
    lines
        .iter()
        .map(|line| escape_html(line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#x27;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

fn mib_value(bytes: u64) -> f64 {
    bytes as f64 / 1_048_576.0
}

fn kib_value(bytes: u64) -> f64 {
    bytes as f64 / 1_024.0
}

/// Delete confirmation dialog for the detail page, same shape as the list
/// dialog so the shared script wiring applies.
fn detail_delete_dialog(csrf_token: &str, provider_id: &str, name: &str) -> Markup {
    let action = format!("/containers/{provider_id}/{name}/delete");
    html! {
        dialog id="delete-container" class="w-[90%] max-w-[44rem] rounded-[0.85rem] border border-line bg-surface p-6 text-ink" aria-labelledby="delete-container-title" {
            form method="post" action=(action) class="m-0" {
                input type="hidden" name="csrf_token" value=(csrf_token);
                h2 id="delete-container-title" class="m-0 font-display text-[1.5rem] font-normal" { "Remove container?" }
                p class="mt-3 text-sm leading-relaxed text-muted" {
                    "This stops and removes " strong class="text-ink" data-confirm-name { (name) } ". Attached volumes are preserved."
                }
                label class="mt-4 flex flex-col gap-2 text-[0.8rem] font-bold" {
                    "Type the container name to confirm"
                    input class=(FIELD) name="confirm_name" type="text" autocomplete="off" data-confirm-input aria-describedby="delete-container-help" required;
                }
                p id="delete-container-help" class="mb-0 mt-2 text-xs text-muted" { "The name must match exactly. Images and volumes are kept." }
                div class="mt-5 flex flex-wrap gap-2" {
                    button type="button" class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-ink" data-dialog-close { "Cancel" }
                    button type="submit" class="rounded-lg border border-danger/40 bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-danger" data-confirm-submit disabled { "Remove container" }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_lines_are_escaped_and_newline_joined() {
        let lines = vec![
            "listening on :8080".to_owned(),
            "<script>alert(1)</script>".to_owned(),
            "a & b \"quoted\" 'single'".to_owned(),
        ];
        let rendered = escape_log_lines(&lines);
        assert_eq!(
            rendered,
            "listening on :8080\n&lt;script&gt;alert(1)&lt;/script&gt;\na &amp; b &quot;quoted&quot; &#x27;single&#x27;"
        );
        assert!(escape_log_lines(&[]).is_empty());
    }

    #[test]
    fn stats_section_formats_memory_and_percentages() {
        let stats = DockerContainerUsage {
            cpu_percent: 12.5,
            memory_used_bytes: 1_048_576,
            memory_limit_bytes: 8_388_608,
            network_rx_bytes: 4096,
            network_tx_bytes: 8192,
        };
        let markup = container_stats_section("demo-api", &stats).into_string();
        assert!(markup.contains("12.5 %"));
        assert!(markup.contains("1.0 MiB"));
        assert!(markup.contains("8.0 MiB"));
        assert!(markup.contains("12.5 %"));
        assert!(markup.contains("4.0 KiB"));

        let zero = DockerContainerUsage::default();
        let markup = container_stats_section("demo-api", &zero).into_string();
        assert!(markup.contains("0.0 %"));
        assert!(!markup.contains("NaN"));
    }

    #[test]
    fn logs_section_marks_truncation_and_empty_tails() {
        let logs = DockerLogs {
            lines: vec!["one".to_owned()],
            truncated: true,
        };
        let markup = container_logs_section("demo-api", &logs).into_string();
        assert!(markup.contains("older output was cut off"));

        let empty = DockerLogs::default();
        let markup = container_logs_section("demo-api", &empty).into_string();
        assert!(markup.contains("No log output."));
    }
}
