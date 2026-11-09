//! CP6 deployment history page: all deployments across environments with
//! per-environment deploy/rollback actions and a 5s HTMX refresh.

use maud::{html, Markup};

use hakobi::catalog::{Deployment, Environment};

use super::{brand, page, sidebar, CONTENT_WIDTH, EYEBROW};

/// A deployment row joined with its environment name.
pub struct DeploymentRowView<'a> {
    pub deployment: &'a Deployment,
    pub environment_name: &'a str,
}

/// Renders a slice of deployment rows as the table body; shared by the page
/// and the 5s HTMX partial.
pub fn deployment_rows(rows: &[DeploymentRowView<'_>]) -> Markup {
    html! {
        div class="overflow-x-auto rounded-[0.85rem] border border-line" {
            @if rows.is_empty() {
                p class="p-6 text-sm text-muted" { "No deployments yet." }
            } @else {
                table class="w-full border-collapse text-left text-sm" {
                    thead class="bg-surface/60 text-[0.65rem] uppercase tracking-[0.12em] text-muted" {
                        tr {
                            th class="px-4 py-3 font-semibold" { "Environment" }
                            th class="px-4 py-3 font-semibold" { "Version" }
                            th class="px-4 py-3 font-semibold" { "Status" }
                            th class="px-4 py-3 font-semibold" { "Trigger" }
                            th class="px-4 py-3 font-semibold" { "Commit" }
                            th class="px-4 py-3 font-semibold" { "Started" }
                            th class="px-4 py-3 font-semibold" { "Completed" }
                        }
                    }
                    tbody {
                        @for row in rows {
                            tr class="border-t border-line" {
                                td class="px-4 py-3 font-semibold" {
                                    a class="text-ink no-underline hover:underline" href=(format!("/environments/{}", row.deployment.environment_id)) { (row.environment_name) }
                                }
                                td class="px-4 py-3" { "v" (row.deployment.version) }
                                td class="px-4 py-3" {
                                    span class=(format!("rounded-full px-2.5 py-1 text-[0.68rem] font-semibold {}", status_badge_class(&row.deployment.status))) { (row.deployment.status) }
                                }
                                td class="px-4 py-3 text-muted" { (trigger_label(&row.deployment.trigger_type)) }
                                td class="px-4 py-3 font-mono text-xs text-muted" {
                                    @if let Some(sha) = &row.deployment.git_commit_sha {
                                        (sha.chars().take(7).collect::<String>())
                                    } @else {
                                        "—"
                                    }
                                }
                                td class="px-4 py-3 text-xs text-muted" { (timestamp(&row.deployment.started_at)) }
                                td class="px-4 py-3 text-xs text-muted" {
                                    @if let Some(completed) = &row.deployment.completed_at {
                                        (timestamp(completed))
                                    } @else {
                                        "—"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn status_badge_class(status: &str) -> &'static str {
    match status {
        "running" => "bg-emerald-500/10 text-emerald-700",
        "building" | "deploying" => "bg-sky-500/10 text-sky-700",
        "failed" => "bg-red-500/10 text-red-700",
        "rolled_back" => "bg-amber-500/10 text-amber-700",
        _ => "bg-slate-500/10 text-slate-600",
    }
}

fn trigger_label(trigger: &str) -> &'static str {
    match trigger {
        "webhook" => "Webhook",
        _ => "Manual",
    }
}

/// Renders a catalog timestamp as `YYYY-MM-DD HH:MM` (UTC).
fn timestamp(value: &str) -> String {
    match value.split_once('T') {
        Some((date, rest)) => {
            let time: String = rest.chars().take(5).collect();
            format!("{date} {time}")
        }
        None => value.to_owned(),
    }
}

/// The `/deployment` history page.
pub fn deployment_page(
    base_url: &str,
    username: &str,
    csrf_token: &str,
    rows: &[DeploymentRowView<'_>],
    environments: &[Environment],
    notice: Option<&str>,
) -> Markup {
    page(
        "Deployments · Hakobi",
        html! {
            div class="min-h-screen lg:pl-64" {
                (sidebar("deployment", base_url, username, csrf_token))
                header class="lg:hidden border-b border-line bg-paper/95" {
                    div class=(format!("{CONTENT_WIDTH} flex h-[5.5rem] items-center justify-between gap-4")) {
                        (brand("Hakobi dashboard"))
                        div class="flex items-center gap-4" {
                            a class="text-[0.8rem] font-semibold text-muted no-underline hover:text-ink" href="/deployment" { "Deployments" }
                            span class="text-sm text-muted max-sm:hidden" { (username) }
                            form method="post" action="/logout" class="m-0" {
                                input type="hidden" name="csrf_token" value=(csrf_token);
                                button class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.8rem] font-semibold text-ink" type="submit" { "Sign out" }
                            }
                        }
                    }
                }
                main class=(format!("{CONTENT_WIDTH} pb-20 pt-10 lg:pt-16")) {
                    div class="mb-6" {
                        p class=(format!("{EYEBROW} mb-3 text-brand-bright")) { "Pipeline" }
                        h1 class="m-0 font-display text-[clamp(2.2rem,5vw,4rem)] font-normal tracking-[-0.04em]" { "Deployments" }
                    }
                    @if let Some(message) = notice {
                        p class="mb-6 rounded-[0.85rem] border border-line bg-surface/60 px-4 py-3 text-sm text-ink" { (message) }
                    }
                    (environment_actions(environments, csrf_token))
                    section class="mt-10" {
                        div class="mb-3 flex items-end justify-between" {
                            h2 class="m-0 text-[1.2rem] font-bold" { "Recent deployments" }
                            span class="text-[0.7rem] text-muted" { "Auto-refreshes every 5s" }
                        }
                        div id="deployment-rows" hx-get="/deployment/list" hx-trigger="every 5s" hx-swap="innerHTML" {
                            (deployment_rows(rows))
                        }
                    }
                }
            }
        },
    )
}

/// Per-environment deploy/rollback action cards.
fn environment_actions(environments: &[Environment], csrf_token: &str) -> Markup {
    html! {
        section class="grid gap-4 md:grid-cols-2" {
            @if environments.is_empty() {
                p class="rounded-[0.85rem] border border-dashed border-line p-6 text-sm text-muted md:col-span-2" {
                    "No environments yet. Create one under Environments to deploy."
                }
            }
            @for environment in environments {
                article class="rounded-[0.85rem] border border-line bg-surface/45 p-5" {
                    div class="flex items-center justify-between gap-3" {
                        div {
                            h3 class="m-0 text-[0.95rem] font-bold" {
                                a class="text-ink no-underline hover:underline" href=(format!("/environments/{}", environment.id)) { (environment.name) }
                            }
                            p class="m-0 mt-1 text-xs text-muted" { (environment.build_type) " · " (environment.status) }
                        }
                        span class=(format!("rounded-full px-2.5 py-1 text-[0.68rem] font-semibold {}", status_badge_class(&environment.status))) { (environment.status) }
                    }
                    div class="mt-4 flex flex-wrap items-center gap-2" {
                        form method="post" action=(format!("/environments/{}/deploy", environment.id)) class="m-0" {
                            input type="hidden" name="csrf_token" value=(csrf_token);
                            button class="rounded-lg bg-brand px-3.5 py-2 text-[0.78rem] font-semibold text-white transition hover:bg-[#0f4130]" type="submit" {
                                i class="fa-solid fa-rocket mr-2" aria-hidden="true" {}
                                "Deploy"
                            }
                        }
                        form method="post" action=(format!("/environments/{}/rollback", environment.id)) class="m-0" {
                            input type="hidden" name="csrf_token" value=(csrf_token);
                            button class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-ink" type="submit" {
                                i class="fa-solid fa-rotate-left mr-2" aria-hidden="true" {}
                                "Rollback"
                            }
                        }
                        a class="text-[0.75rem] font-semibold text-brand no-underline hover:underline" href=(format!("/environments/{}/webhook", environment.id)) { "Webhook" }
                    }
                }
            }
        }
    }
}

/// Webhook configuration page for one environment.
pub fn environment_webhook(
    base_url: &str,
    username: &str,
    csrf_token: &str,
    environment: &Environment,
) -> Markup {
    page(
        "Webhook · Hakobi",
        html! {
            div class="min-h-screen lg:pl-64" {
                (sidebar("environments", base_url, username, csrf_token))
                header class="lg:hidden border-b border-line bg-paper/95" {
                    div class=(format!("{CONTENT_WIDTH} flex h-[5.5rem] items-center justify-between gap-4")) {
                        (brand("Hakobi dashboard"))
                        div class="flex items-center gap-4" {
                            a class="text-[0.8rem] font-semibold text-muted no-underline hover:text-ink" href="/environments" { "Environments" }
                            span class="text-sm text-muted max-sm:hidden" { (username) }
                            form method="post" action="/logout" class="m-0" {
                                input type="hidden" name="csrf_token" value=(csrf_token);
                                button class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.8rem] font-semibold text-ink" type="submit" { "Sign out" }
                            }
                        }
                    }
                }
                main class=(format!("{CONTENT_WIDTH} pb-20 pt-10 lg:pt-16")) {
                    a class="text-[0.75rem] font-semibold text-muted no-underline hover:text-ink" href=(format!("/environments/{}", environment.id)) { "← " (environment.name) }
                    div class="mt-6 mb-8" {
                        p class=(format!("{EYEBROW} mb-3 text-brand-bright")) { "Triggers" }
                        h1 class="m-0 font-display text-[clamp(2rem,4vw,3rem)] font-normal tracking-[-0.04em]" { "Webhook" }
                    }
                    section class="max-w-xl rounded-[0.85rem] border border-line bg-surface/45 p-6" {
                        form method="post" action=(format!("/environments/{}/webhook", environment.id)) {
                            input type="hidden" name="csrf_token" value=(csrf_token);
                            label class="block text-sm font-semibold text-ink" for="webhook_secret" { "Webhook secret" }
                            input class="mt-2 w-full rounded-[0.6rem] border border-line bg-surface px-4 py-3 font-mono text-sm text-ink outline-none transition focus:border-brand-bright focus:ring-[3px] focus:ring-brand-bright/15" type="text" id="webhook_secret" name="webhook_secret" value=(environment.webhook_secret.as_deref().unwrap_or("")) autocomplete="off" {};
                            p class="mt-2 text-xs text-muted" { "POSTed to " span class="font-mono" { (format!("/webhook/{}/<secret>", environment.name)) } " by GitHub, GitLab, or any CI." }
                            label class="mt-4 flex items-center gap-2 text-sm text-ink" {
                                input type="checkbox" name="webhook_enabled" checked[environment.webhook_enabled] {};
                                "Enabled"
                            }
                            button class="mt-5 rounded-lg bg-brand px-4 py-2.5 text-[0.8rem] font-semibold text-white transition hover:bg-[#0f4130]" type="submit" { "Save webhook" }
                        }
                    }
                }
            }
        },
    )
}
