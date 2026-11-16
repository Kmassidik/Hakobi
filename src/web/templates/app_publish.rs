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
pub fn app_status_body(app: &App, latest: Option<&Deployment>) -> Markup {
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
        }
    }
}

pub fn app_publication(app: &App, csrf_token: &str, domain: Option<&str>) -> Markup {
    app_publication_notice(app, csrf_token, domain, None, None, false, false)
}

#[allow(clippy::too_many_arguments)]
pub fn app_publication_notice(
    app: &App,
    csrf_token: &str,
    domain: Option<&str>,
    notice: Option<&str>,
    attempted: Option<&str>,
    is_error: bool,
    full_page: bool,
) -> Markup {
    let publication_url = format!("/apps/{}/publication", app.id);
    let trigger = if app.display_status() == "running" {
        "none"
    } else {
        "every 5s"
    };
    let content = html! {
        div id="app-publication" hx-get=(publication_url) hx-trigger=(trigger) hx-swap="outerHTML" {
            (publication_panel(app, csrf_token, domain, notice, attempted, is_error))
        }
    };
    if full_page {
        super::page(
            "Publish application · Hakobi",
            html! { main class="mx-auto w-[90%] max-w-2xl py-12" {
                a class="text-sm font-semibold text-brand" href=(format!("/apps/{}", app.id)) { "← Back to application" }
                h1 class="mb-4 mt-6 font-display text-4xl font-normal" { (app.name) }
                (content)
            } },
        )
    } else {
        content
    }
}

fn publication_panel(
    app: &App,
    csrf_token: &str,
    domain: Option<&str>,
    notice: Option<&str>,
    attempted: Option<&str>,
    is_error: bool,
) -> Markup {
    html! {
        @if let Some(message) = notice {
            p id="publication-message" class=(if is_error { "mt-4 text-sm font-semibold text-danger" } else { "mt-4 text-sm font-semibold text-brand" }) role=(if is_error { "alert" } else { "status" }) { (message) }
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
                        (publish_form(app, csrf_token, domain, attempted))
                    }
                }
            }
        }
    }
}

fn publish_form(app: &App, csrf_token: &str, domain: &str, attempted: Option<&str>) -> Markup {
    let draft = attempted
        .map(str::to_owned)
        .unwrap_or_else(|| default_subdomain(&app.name));
    let url = format!("/apps/{}/publish", app.id);
    let availability_url = format!("/apps/{}/subdomain", app.id);
    html! {
        form method="post" action=(url.clone()) hx-post=(url) hx-target="#app-publication" hx-swap="outerHTML" data-subdomain-availability-form="" data-availability-url=(availability_url) class="flex flex-wrap items-end gap-3" {
            input type="hidden" name="csrf_token" value=(csrf_token);
            label class="grid gap-1.5 text-sm font-semibold" { "Subdomain"
                span class="flex items-center gap-2" {
                    input name="subdomain" value=(draft) pattern="[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?" maxlength="63" required data-subdomain-input="" aria-describedby="publication-message subdomain-availability" class="rounded-lg border border-line bg-surface px-3 py-2.5 text-sm";
                    span class="text-sm font-normal text-muted" { "." (domain) }
                }
            }
            button type="submit" data-subdomain-submit="" class="rounded-lg bg-brand px-4 py-2.5 text-sm font-bold text-white" { "Publish application" }
            p id="subdomain-availability" data-subdomain-availability-status="" role="status" aria-live="polite" class="m-0 min-h-5 w-full text-sm text-muted" {}
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
