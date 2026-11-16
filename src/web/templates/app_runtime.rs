//! Runtime controls and live usage charts for one app.

use maud::{html, Markup};

use hakobi::catalog::App;

pub(crate) fn app_runtime_card(app: &App, csrf_token: &str) -> Markup {
    let status = app.display_status();
    html! {
        article id="app-runtime" hx-get=(format!("/apps/{}/runtime", app.id)) hx-trigger="every 5s" hx-swap="outerHTML" class="rounded-[0.85rem] border border-line bg-surface/45 p-5" {
            h2 class="m-0 mb-3 text-[0.95rem] font-bold" { "Runtime" }
            p class="m-0 mb-3 text-sm text-muted" { "Container state: " span class="font-semibold text-ink" { (status) } }
            div class="flex flex-wrap gap-2" {
                @if status != "running" && status != "paused" {
                    (runtime_button(app, csrf_token, "start", "Start", "fa-play"))
                }
                @if status == "running" {
                    (runtime_button(app, csrf_token, "stop", "Stop", "fa-stop"))
                    (runtime_button(app, csrf_token, "restart", "Restart", "fa-rotate-right"))
                    (runtime_button(app, csrf_token, "pause", "Pause", "fa-pause"))
                }
                @if status == "paused" {
                    (runtime_button(app, csrf_token, "unpause", "Resume", "fa-play"))
                }
            }
            p class="mb-0 mt-3 text-xs text-muted" {
                "Pause freezes processes without removing the container. Stop shuts it down. Deploy replaces it."
            }
        }
    }
}

fn runtime_button(app: &App, csrf_token: &str, action: &str, label: &str, icon: &str) -> Markup {
    html! {
        form method="post" action=(format!("/apps/{}/runtime/{action}", app.id)) class="m-0" {
            input type="hidden" name="csrf_token" value=(csrf_token);
            button class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-ink" type="submit" {
                i class=(format!("fa-solid {icon} mr-2")) aria-hidden="true" {}
                (label)
            }
        }
    }
}

pub(crate) fn app_usage_section(app: &App) -> Markup {
    html! {
        section id="app-usage" class="mt-8" data-app-usage=(format!("/apps/{}/usage", app.id)) aria-label="Live app resource usage" {
            div class="mb-3 flex items-end justify-between gap-3" {
                h2 class="m-0 text-[1.2rem] font-bold" { "Resource usage" }
                span class="text-xs text-muted" data-app-usage-status { "Waiting for the first live sample…" }
            }
            div class="grid grid-cols-1 gap-4 lg:grid-cols-2" {
                (usage_chart("CPU usage (%)", "cpu"))
                (usage_chart("Memory usage (MiB)", "memory"))
            }
        }
    }
}

fn usage_chart(title: &str, kind: &str) -> Markup {
    html! {
        article class="rounded-[0.85rem] border border-line bg-surface/45 p-5" {
            h3 class="m-0 mb-3 text-[0.85rem] font-semibold" { (title) }
            canvas class="h-[12rem] w-full" data-chart="auto" data-window="5m" data-usage-kind=(kind) data-series="[]" aria-label=(title) {}
        }
    }
}
