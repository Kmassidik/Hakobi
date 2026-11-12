//! CP9 app templates: list page, detail page, create/edit form, status and
//! logs partials, and the shared deployment-table rows.

use maud::{html, Markup};

use hakobi::catalog::{App, Deployment, DeploymentLog};

use super::{brand, page, sidebar, CONTENT_WIDTH, EYEBROW};

fn app_status_class(status: &str) -> &'static str {
    match status {
        "running" => "bg-emerald-500/10 text-emerald-700",
        "building" | "deploying" => "bg-sky-500/10 text-sky-700",
        "failed" => "bg-red-500/10 text-red-700",
        _ => "bg-slate-500/10 text-slate-600",
    }
}

fn deployment_status_class(status: &str) -> &'static str {
    match status {
        "running" => "bg-emerald-500/10 text-emerald-700",
        "building" | "deploying" => "bg-sky-500/10 text-sky-700",
        "failed" => "bg-red-500/10 text-red-700",
        "rolled_back" => "bg-amber-500/10 text-amber-700",
        _ => "bg-slate-500/10 text-slate-600",
    }
}

/// Apps list page: one card per app with status dot, name, repo, and branch.
pub fn apps(
    base_url: &str,
    username: &str,
    csrf_token: &str,
    apps: &[App],
    notice: Option<&str>,
) -> Markup {
    page(
        "Apps · Hakobi",
        html! {
            div class="min-h-screen lg:pl-60" {
                (sidebar("apps", base_url, username, csrf_token))
                header class="lg:hidden border-b border-line bg-paper/95" {
                    div class=(format!("{CONTENT_WIDTH} flex h-[5.5rem] items-center justify-between gap-4")) {
                        (brand("Hakobi dashboard"))
                        div class="flex items-center gap-4" {
                            a class="text-[0.8rem] font-semibold text-muted no-underline hover:text-ink" href="/dashboard" { "Overview" }
                            span class="text-sm text-muted max-sm:hidden" { (username) }
                            form method="post" action="/logout" class="m-0" {
                                input type="hidden" name="csrf_token" value=(csrf_token);
                                button class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.8rem] font-semibold text-ink" type="submit" { "Sign out" }
                            }
                        }
                    }
                }
                main class=(format!("{CONTENT_WIDTH} pb-20 pt-10 lg:pt-16")) {
                    div class="flex flex-col items-start gap-4 sm:flex-row sm:items-end sm:justify-between" {
                        div {
                            p class=(format!("{EYEBROW} mb-3 text-brand-bright")) { "Delivery targets" }
                            h1 class="m-0 font-display text-[clamp(2.2rem,5vw,4rem)] font-normal tracking-[-0.04em]" { "Apps" }
                        }
                        a class="rounded-[0.6rem] bg-brand px-4 py-3 text-[0.85rem] font-bold text-white no-underline transition hover:bg-[#0f4130]" href="/apps/new" {
                            i class="fa-solid fa-plus mr-2" aria-hidden="true" {}
                            "New App"
                        }
                    }
                    p class="mt-4 max-w-[44rem] leading-[1.7] text-muted" {
                        "Each app binds a build recipe to one enrolled host. Pushes to the tracked branch deploy automatically when auto-deploy is on."
                    }
                    @if let Some(message) = notice {
                        p class="mb-6 mt-6 rounded-[0.6rem] border border-line bg-surface/60 px-4 py-3 text-sm" role="status" { (message) }
                    }
                    @if apps.is_empty() {
                        div class="mt-8 rounded-[0.85rem] border border-dashed border-line p-8 text-center text-muted" {
                            img src="/assets/art/illustrations/empty-stacks.webp" alt="" aria-hidden="true" class="empty-art";
                            p class="m-0 mb-3" { "No apps yet." }
                            a class="rounded-[0.6rem] bg-brand px-4 py-2.5 text-[0.8rem] font-bold text-white no-underline transition hover:bg-[#0f4130]" href="/apps/new" {
                                "Create your first app"
                            }
                        }
                    } @else {
                        div class="mt-8 grid gap-4" {
                            @for app in apps {
                                article class="rounded-[0.85rem] border border-line bg-surface/45 p-6" {
                                    div class="flex flex-col justify-between gap-3 sm:flex-row sm:items-start" {
                                        div {
                                            h2 class="m-0 flex items-center gap-2.5 text-[1.1rem] font-bold" {
                                                span class=(format!("size-2 shrink-0 rounded-full {}", match app.status.as_str() {
                                                    "running" => "bg-emerald-500",
                                                    "building" | "deploying" => "bg-sky-500 animate-pulse",
                                                    "failed" => "bg-red-500",
                                                    _ => "bg-slate-400",
                                                })) aria-hidden="true" {}
                                                a class="no-underline text-inherit hover:underline" href=(format!("/apps/{}", app.id)) { (app.name) }
                                            }
                                            p class="mb-0 mt-1 text-xs text-muted" {
                                                @if let Some(repo) = &app.github_repo {
                                                    span class="font-mono text-ink" { (repo) }
                                                    @if let Some(branch) = &app.github_branch {
                                                        " · " (branch)
                                                    }
                                                } @else {
                                                    "No repository linked"
                                                }
                                                " · build: " (app.build_type)
                                            }
                                        }
                                        span class=(format!("inline-flex w-fit items-center gap-2 rounded-full px-3 py-1 text-xs font-semibold {}", app_status_class(&app.status))) {
                                            (app.status)
                                        }
                                    }
                                    div class="mt-4 flex flex-wrap gap-2" {
                                        a class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-ink no-underline" href=(format!("/apps/{}", app.id)) { "Open" }
                                        a class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-ink no-underline" href=(format!("/apps/{}/edit", app.id)) { "Edit" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        },
    )
}

/// App detail page: deploy button, deployments table with rollback, webhook
/// info, and the live status strip.
#[allow(clippy::too_many_arguments)]
pub fn app_detail(
    base_url: &str,
    username: &str,
    csrf_token: &str,
    app: &App,
    deployments: &[Deployment],
    latest: Option<&Deployment>,
    previous: Option<&Deployment>,
) -> Markup {
    page(
        "App · Hakobi",
        html! {
            div class="min-h-screen lg:pl-60" {
                (sidebar("apps", base_url, username, csrf_token))
                header class="lg:hidden border-b border-line bg-paper/95" {
                    div class=(format!("{CONTENT_WIDTH} flex h-[5.5rem] items-center justify-between gap-4")) {
                        (brand("Hakobi dashboard"))
                        div class="flex items-center gap-4" {
                            a class="text-[0.8rem] font-semibold text-muted no-underline hover:text-ink" href="/apps" { "Apps" }
                            span class="text-sm text-muted max-sm:hidden" { (username) }
                            form method="post" action="/logout" class="m-0" {
                                input type="hidden" name="csrf_token" value=(csrf_token);
                                button class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.8rem] font-semibold text-ink" type="submit" { "Sign out" }
                            }
                        }
                    }
                }
                main class=(format!("{CONTENT_WIDTH} pb-20 pt-10 lg:pt-16")) {
                    a class="text-[0.75rem] font-semibold text-muted no-underline hover:text-ink" href="/apps" { "← All apps" }
                    div class="mt-6 flex flex-col items-start gap-4 sm:flex-row sm:items-end sm:justify-between" {
                        div {
                            p class=(format!("{EYEBROW} mb-3 text-brand-bright")) { "App" }
                            h1 class="m-0 font-display text-[clamp(2.2rem,5vw,4rem)] font-normal tracking-[-0.04em]" { (app.name) }
                        }
                        div class="flex flex-wrap items-center gap-2" {
                            a class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-ink no-underline" href=(format!("/apps/{}/edit", app.id)) {
                                i class="fa-solid fa-pen mr-2" aria-hidden="true" {}
                                "Edit"
                            }
                            button type="button" class="rounded-lg border border-danger/40 bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-danger" data-dialog-open="delete-app" data-delete-action=(format!("/apps/{}/delete", app.id)) data-delete-name=(app.name) {
                                i class="fa-solid fa-trash-can mr-2" aria-hidden="true" {}
                                "Delete"
                            }
                        }
                    }
                    (super::app_form::delete_app_dialog(csrf_token))
                    (app_status_body(app, latest))
                    div class="mt-6 grid gap-4 md:grid-cols-2" {
                        article class="rounded-[0.85rem] border border-line bg-surface/45 p-5" {
                            h2 class="m-0 mb-3 text-[0.95rem] font-bold" { "Build" }
                            p class="m-0 text-sm text-muted" { "Build type: " span class="font-semibold text-ink" { (app.build_type) } }
                            @if let Some(path) = &app.github_path {
                                p class="m-0 mt-1 text-sm text-muted" { "Context path: " span class="font-mono text-xs text-ink" { (path) } }
                            }
                            @if let Some(dockerfile) = &app.github_dockerfile {
                                p class="m-0 mt-1 text-sm text-muted" { "Dockerfile: " span class="font-mono text-xs text-ink" { (dockerfile) } }
                            }
                            @if let Some(port) = app.container_port {
                                p class="m-0 mt-1 text-sm text-muted" { "Container port: " span class="font-semibold text-ink" { (port) } }
                            }
                        }
                        article class="rounded-[0.85rem] border border-line bg-surface/45 p-5" {
                            h2 class="m-0 mb-3 text-[0.95rem] font-bold" { "Source" }
                            @if let Some(repo) = &app.github_repo {
                                p class="m-0 break-all text-sm text-muted" { "Repo: " span class="font-mono text-xs text-ink" { (repo) } }
                                @if let Some(branch) = &app.github_branch {
                                    p class="m-0 mt-1 text-sm text-muted" { "Branch: " span class="font-semibold text-ink" { (branch) } }
                                }
                                span class=(format!("mt-2 inline-flex items-center gap-1 rounded-full px-2.5 py-1 text-[0.68rem] font-semibold {}", if app.github_auto_deploy { "bg-brand-bright/10 text-brand" } else { "bg-slate-500/10 text-slate-600" })) {
                                    @if app.github_auto_deploy { "Auto-deploy on push" } @else { "Manual deploys only" }
                                }
                            } @else {
                                p class="m-0 text-sm text-muted" { "No repository linked yet." }
                            }
                        }
                        article class="rounded-[0.85rem] border border-line bg-surface/45 p-5" {
                            h2 class="m-0 mb-3 text-[0.95rem] font-bold" { "Webhook" }
                            @if let Some(secret) = &app.webhook_secret {
                                p class="m-0 break-all font-mono text-xs text-ink" { (format!("{}/webhook/{}/{}", base_url, app.id, secret)) }
                                p class="m-0 mt-2 text-xs text-muted" { "POST a JSON payload with a commit SHA to deploy." }
                            } @else {
                                p class="m-0 text-sm text-muted" { "No webhook secret configured. Set one on the edit page to enable external triggers." }
                            }
                        }
                        article class="rounded-[0.85rem] border border-line bg-surface/45 p-5" {
                            h2 class="m-0 mb-3 text-[0.95rem] font-bold" { "Deploy" }
                            form method="post" action=(format!("/apps/{}/deploy", app.id)) class="m-0" {
                                input type="hidden" name="csrf_token" value=(csrf_token);
                                button class="rounded-lg bg-brand px-3.5 py-2 text-[0.78rem] font-semibold text-white transition hover:bg-[#0f4130]" type="submit" {
                                    i class="fa-solid fa-rocket mr-2" aria-hidden="true" {}
                                    "Deploy"
                                }
                            }
                            @if let Some(previous) = previous {
                                form method="post" action=(format!("/apps/{}/rollback", app.id)) class="mt-2" {
                                    input type="hidden" name="csrf_token" value=(csrf_token);
                                    button class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-ink" type="submit" {
                                        i class="fa-solid fa-rotate-left mr-2" aria-hidden="true" {}
                                        "Rollback to v" (previous.version)
                                    }
                                }
                            }
                            p class="mb-0 mt-3 text-xs text-muted" { "Deployments run through the build runner; watch progress in the history below." }
                        }
                    }
                    section id="app-deployments" class="mt-10" {
                        div class="mb-3 flex flex-col gap-1 sm:flex-row sm:items-end sm:justify-between" {
                            h2 class="m-0 text-[1.2rem] font-bold" { "Deployment history" }
                            a class="text-[0.78rem] font-semibold text-brand no-underline hover:underline" href=(format!("/apps/{}/logs", app.id)) { "Latest logs →" }
                        }
                        (deployment_rows(app, deployments, csrf_token))
                    }
                }
            }
        },
    )
}

/// Deployment table for one app: version, status, trigger, commit, and
/// rollback/cancel actions. Rendered fresh on the HTMX refresh partial.
pub fn deployment_rows(app: &App, deployments: &[Deployment], csrf_token: &str) -> Markup {
    if deployments.is_empty() {
        html! {
            p class="rounded-[0.85rem] border border-dashed border-line p-6 text-sm text-muted" { "No deployments yet." }
        }
    } else {
        html! {
            div class="overflow-x-auto rounded-[0.85rem] border border-line" {
                table class="w-full border-collapse text-left text-sm" {
                    thead class="bg-surface/60 text-[0.65rem] uppercase tracking-[0.12em] text-muted" {
                        tr {
                            th class="px-4 py-3 font-semibold" { "Version" }
                            th class="px-4 py-3 font-semibold" { "Status" }
                            th class="px-4 py-3 font-semibold" { "Trigger" }
                            th class="px-4 py-3 font-semibold" { "Commit" }
                            th class="px-4 py-3 font-semibold" { "Started" }
                            th class="px-4 py-3 font-semibold" { "Completed" }
                            th class="px-4 py-3 font-semibold" { "" }
                        }
                    }
                    tbody {
                        @for deployment in deployments.iter().take(10) {
                            tr class="border-t border-line" {
                                td class="px-4 py-3 font-semibold" { "v" (deployment.version) }
                                td class="px-4 py-3" {
                                    span class=(format!("rounded-full px-2.5 py-1 text-[0.68rem] font-semibold {}", deployment_status_class(&deployment.status))) { (deployment.status) }
                                }
                                td class="px-4 py-3 text-muted" { (deployment.trigger_type) }
                                td class="px-4 py-3 font-mono text-xs text-muted" { (deployment.git_commit_sha.as_deref().unwrap_or("—")) }
                                td class="px-4 py-3 text-xs text-muted" { (deployment.started_at) }
                                td class="px-4 py-3 text-xs text-muted" { (deployment.completed_at.as_deref().unwrap_or("—")) }
                                td class="px-4 py-3" {
                                    div class="flex flex-wrap gap-2" {
                                        @if deployment.status == "running" {
                                            form method="post" action=(format!("/apps/{}/rollback", app.id)) class="m-0" {
                                                input type="hidden" name="csrf_token" value=(csrf_token);
                                                button class="rounded-lg border border-line bg-transparent px-2.5 py-1.5 text-[0.68rem] font-semibold text-ink" type="submit" { "Rollback" }
                                            }
                                        }
                                        @if matches!(deployment.status.as_str(), "pending" | "building" | "deploying") {
                                            form method="post" action=(format!("/apps/{}/cancel", deployment.id)) class="m-0" {
                                                input type="hidden" name="csrf_token" value=(csrf_token);
                                                button class="rounded-lg border border-danger/40 bg-transparent px-2.5 py-1.5 text-[0.68rem] font-semibold text-danger" type="submit" { "Cancel" }
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
    }
}

/// Status strip shared by the detail page and the 5s HTMX partial.
pub fn app_status_body(app: &App, latest: Option<&Deployment>) -> Markup {
    html! {
        section id="app-status" hx-get=(format!("/apps/{}/status", app.id)) hx-trigger="every 5s" hx-swap="outerHTML" {
            div class="mt-6 flex flex-wrap items-center gap-3 rounded-[0.85rem] border border-line bg-surface/45 px-5 py-4" {
                span class=(format!("rounded-full px-3 py-1 text-xs font-semibold {}", app_status_class(&app.status))) {
                    (app.status)
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

/// Deployments-only page for one app (GET /apps/{id}/deployments is a
/// partial; this full page anchors it).
pub fn app_deployments(
    base_url: &str,
    username: &str,
    csrf_token: &str,
    app: &App,
    deployments: &[Deployment],
) -> Markup {
    page(
        "Deployments · Hakobi",
        html! {
            div class="min-h-screen lg:pl-60" {
                (sidebar("apps", base_url, username, csrf_token))
                header class="lg:hidden border-b border-line bg-paper/95" {
                    div class=(format!("{CONTENT_WIDTH} flex h-[5.5rem] items-center justify-between gap-4")) {
                        (brand("Hakobi dashboard"))
                        div class="flex items-center gap-4" {
                            a class="text-[0.8rem] font-semibold text-muted no-underline hover:text-ink" href="/apps" { "Apps" }
                            span class="text-sm text-muted max-sm:hidden" { (username) }
                            form method="post" action="/logout" class="m-0" {
                                input type="hidden" name="csrf_token" value=(csrf_token);
                                button class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.8rem] font-semibold text-ink" type="submit" { "Sign out" }
                            }
                        }
                    }
                }
                main class=(format!("{CONTENT_WIDTH} pb-20 pt-10 lg:pt-16")) {
                    a class="text-[0.75rem] font-semibold text-muted no-underline hover:text-ink" href=(format!("/apps/{}", app.id)) { "← " (app.name) }
                    p class=(format!("{EYEBROW} mb-3 mt-6 text-brand-bright")) { "History" }
                    h1 class="m-0 font-display text-[clamp(2.2rem,5vw,4rem)] font-normal tracking-[-0.04em]" { "Deployments" }
                    div class="mt-8" {
                        (deployment_rows(app, deployments, csrf_token))
                    }
                }
            }
        },
    )
}

/// Live log page for the latest deployment of one app.
pub fn app_logs(
    base_url: &str,
    username: &str,
    csrf_token: &str,
    app: &App,
    deployment: Option<&Deployment>,
    logs: &[DeploymentLog],
) -> Markup {
    page(
        "Deployment logs · Hakobi",
        html! {
            div class="min-h-screen lg:pl-60" {
                (sidebar("apps", base_url, username, csrf_token))
                header class="lg:hidden border-b border-line bg-paper/95" {
                    div class=(format!("{CONTENT_WIDTH} flex h-[5.5rem] items-center justify-between gap-4")) {
                        (brand("Hakobi dashboard"))
                        div class="flex items-center gap-4" {
                            a class="text-[0.8rem] font-semibold text-muted no-underline hover:text-ink" href="/apps" { "Apps" }
                            span class="text-sm text-muted max-sm:hidden" { (username) }
                            form method="post" action="/logout" class="m-0" {
                                input type="hidden" name="csrf_token" value=(csrf_token);
                                button class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.8rem] font-semibold text-ink" type="submit" { "Sign out" }
                            }
                        }
                    }
                }
                main class=(format!("{CONTENT_WIDTH} pb-20 pt-10 lg:pt-16")) {
                    a class="text-[0.75rem] font-semibold text-muted no-underline hover:text-ink" href=(format!("/apps/{}", app.id)) { "← " (app.name) }
                    p class=(format!("{EYEBROW} mb-3 mt-6 text-brand-bright")) { "Output" }
                    h1 class="m-0 font-display text-[clamp(2.2rem,5vw,4rem)] font-normal tracking-[-0.04em]" { "Deployment logs" }
                    @match deployment {
                        Some(deployment) => p class="mt-3 text-sm text-muted" { "Latest: v" (deployment.version) " · " (deployment.status) },
                        None => p class="mt-3 text-sm text-muted" { "No deployments yet." },
                    }
                    div id="app-logs" class="mt-8 max-h-[36rem] overflow-auto rounded-[0.85rem] border border-line bg-surface p-5 font-mono text-xs" {
                        @if logs.is_empty() {
                            p class="m-0 text-muted" { "No log lines recorded." }
                        } @else {
                            @for log in logs.iter().rev().take(200).collect::<Vec<_>>().iter().rev() {
                                p class="m-0 leading-relaxed" {
                                    span class="text-muted" { (log.timestamp) " " }
                                    span class=(match log.level.as_str() {
                                        "error" => "text-red-600 font-semibold",
                                        "warn" => "text-amber-600 font-semibold",
                                        _ => "text-brand",
                                    }) { (log.level) }
                                    " " (log.message)
                                }
                            }
                        }
                    }
                }
            }
        },
    )
}
