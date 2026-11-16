//! Post-build publication controls; public networking remains tunnel/edge-only.

use hakobi::catalog::{App, Deployment};
use maud::{html, Markup};

pub(super) fn app_status_class(status: &str) -> &'static str {
    match status {
        "running" => "bg-emerald-500/10 text-emerald-700",
        "building" | "deploying" => "bg-sky-500/10 text-sky-700",
        "failed" => "bg-red-500/10 text-red-700",
        "paused" => "bg-amber-500/10 text-amber-700",
        _ => "bg-slate-500/10 text-slate-600",
    }
}

/// Status strip shared by the detail page and the five-second HTMX partial.
pub fn app_status_body(
    app: &App,
    latest: Option<&Deployment>,
    csrf_token: &str,
    apps_domain: Option<&str>,
) -> Markup {
    html! {
        section id="app-status" hx-get=(format!("/apps/{}/status", app.id)) hx-trigger="every 5s" hx-swap="outerHTML" {
            div class="mt-6 flex flex-wrap items-center gap-3 rounded-[0.85rem] border border-line bg-surface/45 px-5 py-4" {
                span class=(format!("rounded-full px-3 py-1 text-xs font-semibold {}", app_status_class(app.display_status()))) {
                    (app.display_status())
                }
                @match latest {
                    Some(deployment) => span class="text-sm text-muted" {
                        "Latest deployment: v" (deployment.version) " · "
                        span class="font-semibold" { (deployment.status) }
                        " · " (deployment.trigger_type)
                    },
                    None => span class="text-sm text-muted" { "No deployments yet." },
                }
            }
            (app_publication(app, csrf_token, apps_domain))
        }
    }
}

pub fn app_publication(app: &App, csrf_token: &str, domain: Option<&str>) -> Markup {
    app_publication_notice(app, csrf_token, domain, None)
}

pub fn app_publication_notice(
    app: &App,
    csrf_token: &str,
    domain: Option<&str>,
    notice: Option<&str>,
) -> Markup {
    html! {
        div id="app-publication" {
            @if let Some(message) = notice {
                p class="mt-4 text-sm font-semibold text-brand" role="status" { (message) }
            }
            @if let Some(domain) = domain {
                @if app.display_status() == "running" {
                    @if let Some(subdomain) = &app.subdomain {
                        section class="mt-5 border-y border-line py-5" {
                            p class="m-0 text-xs font-semibold uppercase tracking-[0.08em] text-muted" { "Public application" }
                            a class="mt-2 inline-block font-display text-2xl text-brand no-underline" href=(format!("https://{subdomain}.{domain}")) {
                                (subdomain) "." (domain) " ↗"
                            }
                        }
                    } @else {
                        section class="mt-5 border-y border-line py-5" {
                            h2 class="m-0 font-display text-2xl font-normal" { "Your app is running privately." }
                            p class="mb-4 mt-2 text-sm text-muted" { "Choose a subdomain to route it through the tunnel and Hakobi proxy." }
                            form method="post" action=(format!("/apps/{}/publish", app.id)) hx-post=(format!("/apps/{}/publish", app.id)) hx-target="#app-publication" hx-swap="outerHTML" class="flex flex-wrap items-end gap-3" {
                                input type="hidden" name="csrf_token" value=(csrf_token);
                                label class="grid gap-1.5 text-sm font-semibold" { "Subdomain"
                                    span class="flex items-center gap-2" {
                                        input name="subdomain" value=(default_subdomain(&app.name)) pattern="[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?" maxlength="63" required class="rounded-lg border border-line bg-surface px-3 py-2.5 text-sm";
                                        span class="text-sm font-normal text-muted" { "." (domain) }
                                    }
                                }
                                button type="submit" class="rounded-lg bg-brand px-4 py-2.5 text-sm font-bold text-white" { "Publish application" }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn default_subdomain(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|character| {
            let lower = character.to_ascii_lowercase();
            if lower.is_ascii_alphanumeric() || lower == '-' {
                lower
            } else {
                '-'
            }
        })
        .collect();
    slug.trim_matches('-').chars().take(63).collect()
}
