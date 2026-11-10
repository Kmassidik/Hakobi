//! CP5 environment templates: list, detail, form, deployments, and logs.

use maud::{html, Markup};

use hakobi::catalog::{Deployment, DeploymentLog, Environment, HostAgent};

use super::environment_form::delete_environment_dialog;
use super::{brand, page, sidebar, CONTENT_WIDTH, EYEBROW};

fn environment_status_class(status: &str) -> &'static str {
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

pub fn environments(
    base_url: &str,
    username: &str,
    csrf_token: &str,
    environments: &[Environment],
    hosts: &[HostAgent],
    notice: Option<&str>,
) -> Markup {
    page(
        "Environments · Hakobi",
        html! {
            div class="min-h-screen lg:pl-60" {
                (sidebar("environments", base_url, username, csrf_token))
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
                            h1 class="m-0 font-display text-[clamp(2.2rem,5vw,4rem)] font-normal tracking-[-0.04em]" { "Environments" }
                        }
                        a class="rounded-[0.6rem] bg-brand px-4 py-3 text-[0.85rem] font-bold text-white no-underline transition hover:bg-[#0f4130]" href="/environments/new" {
                            i class="fa-solid fa-plus mr-2" aria-hidden="true" {}
                            "New Environment"
                        }
                    }
                    p class="mt-4 max-w-[44rem] leading-[1.7] text-muted" {
                        "Each environment binds a build recipe to one enrolled host. Deploy and rollback run in the pipeline milestone; definitions are editable now."
                    }
                    @if let Some(message) = notice {
                        p class="mb-6 mt-6 rounded-[0.6rem] border border-line bg-surface/60 px-4 py-3 text-sm" role="status" { (message) }
                    }
                    @if environments.is_empty() {
                        div class="mt-8 rounded-[0.85rem] border border-dashed border-line p-8 text-center text-muted" {
                            img src="/assets/art/illustrations/empty-stacks.webp" alt="" aria-hidden="true" class="empty-art";
                            p class="m-0 mb-3" { "No environments yet." }
                            a class="rounded-[0.6rem] bg-brand px-4 py-2.5 text-[0.8rem] font-bold text-white no-underline transition hover:bg-[#0f4130]" href="/environments/new" {
                                "Create your first environment"
                            }
                        }
                    } @else {
                        div class="mt-8 grid gap-4" {
                            @for environment in environments {
                                article class="rounded-[0.85rem] border border-line bg-surface/45 p-6" {
                                    div class="flex flex-col justify-between gap-3 sm:flex-row sm:items-start" {
                                        div {
                                            h2 class="m-0 text-[1.1rem] font-bold" {
                                                a class="no-underline text-inherit hover:underline" href=(format!("/environments/{}", environment.id)) { (environment.name) }
                                            }
                                            p class="mb-0 mt-1 text-xs text-muted" {
                                                "Host: "
                                                span class="font-semibold text-ink" {
                                                    (hosts.iter().find(|host| host.id == environment.host_id).map(|host| host.name.as_str()).unwrap_or(&environment.host_id))
                                                }
                                                " · build: " (environment.build_type) " · source: " (environment.source_type)
                                            }
                                        }
                                        span class=(format!("inline-flex w-fit items-center gap-2 rounded-full px-3 py-1 text-xs font-semibold {}", environment_status_class(&environment.status))) {
                                            (environment.status)
                                        }
                                    }
                                    div class="mt-4 flex flex-wrap gap-2" {
                                        a class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-ink no-underline" href=(format!("/environments/{}", environment.id)) { "Open" }
                                        a class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-ink no-underline" href=(format!("/environments/{}/edit", environment.id)) { "Edit" }
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

#[allow(clippy::too_many_arguments)]
pub fn environment_detail(
    base_url: &str,
    username: &str,
    csrf_token: &str,
    environment: &Environment,
    hosts: &[HostAgent],
    deployments: &[Deployment],
    latest: Option<&Deployment>,
    previous: Option<&Deployment>,
) -> Markup {
    let host_label = hosts
        .iter()
        .find(|host| host.id == environment.host_id)
        .map(|host| host.name.as_str())
        .unwrap_or(&environment.host_id);
    page(
        "Environment · Hakobi",
        html! {
            div class="min-h-screen lg:pl-60" {
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
                    a class="text-[0.75rem] font-semibold text-muted no-underline hover:text-ink" href="/environments" { "← All environments" }
                    div class="mt-6 flex flex-col items-start gap-4 sm:flex-row sm:items-end sm:justify-between" {
                        div {
                            p class=(format!("{EYEBROW} mb-3 text-brand-bright")) { "Environment" }
                            h1 class="m-0 font-display text-[clamp(2.2rem,5vw,4rem)] font-normal tracking-[-0.04em]" { (environment.name) }
                        }
                        div class="flex flex-wrap items-center gap-2" {
                            a class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-ink no-underline" href=(format!("/environments/{}/edit", environment.id)) {
                                i class="fa-solid fa-pen mr-2" aria-hidden="true" {}
                                "Edit"
                            }
                            button type="button" class="rounded-lg border border-danger/40 bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-danger" data-dialog-open="delete-environment" data-delete-action=(format!("/environments/{}/delete", environment.id)) data-delete-name=(environment.name) {
                                i class="fa-solid fa-trash-can mr-2" aria-hidden="true" {}
                                "Delete"
                            }
                        }
                    }
                    (delete_environment_dialog(csrf_token))
                    (environment_status_body(environment, latest))
                    div class="mt-6 grid gap-4 md:grid-cols-2" {
                        article class="rounded-[0.85rem] border border-line bg-surface/45 p-5" {
                            h2 class="m-0 mb-3 text-[0.95rem] font-bold" { "Build" }
                            p class="m-0 text-sm text-muted" { "Build type: " span class="font-semibold text-ink" { (environment.build_type) } }
                            p class="m-0 mt-1 text-sm text-muted" { "Host: " span class="font-semibold text-ink" { (host_label) } }
                        }
                        article class="rounded-[0.85rem] border border-line bg-surface/45 p-5" {
                            h2 class="m-0 mb-3 text-[0.95rem] font-bold" { "Source" }
                            p class="m-0 text-sm text-muted" { "Type: " span class="font-semibold text-ink" { (environment.source_type) } }
                            @if let Some(url) = &environment.git_url {
                                p class="m-0 mt-1 break-all text-sm text-muted" { "Git: " span class="font-mono text-xs text-ink" { (url) } }
                            }
                            @if let Some(branch) = &environment.git_branch {
                                p class="m-0 mt-1 text-sm text-muted" { "Branch: " span class="font-semibold text-ink" { (branch) } }
                            }
                        }
                        article class="rounded-[0.85rem] border border-line bg-surface/45 p-5" {
                            h2 class="m-0 mb-3 text-[0.95rem] font-bold" { "Registry" }
                            p class="m-0 text-sm text-muted" { "Mode: " span class="font-semibold text-ink" { (environment.registry_mode) } }
                            @if environment.registry_mode == "push" {
                                span class="mt-2 inline-flex items-center gap-1 rounded-full bg-brand-bright/10 px-2.5 py-1 text-[0.68rem] font-semibold text-brand" { "Images push to an external registry" }
                            } @else {
                                span class="mt-2 inline-flex items-center gap-1 rounded-full bg-slate-500/10 px-2.5 py-1 text-[0.68rem] font-semibold text-slate-600" { "Images stay on the host" }
                            }
                        }
                        article class="rounded-[0.85rem] border border-line bg-surface/45 p-5" {
                            h2 class="m-0 mb-3 text-[0.95rem] font-bold" { "Deploy" }
                            form method="post" action=(format!("/environments/{}/deploy", environment.id)) class="m-0" {
                                input type="hidden" name="csrf_token" value=(csrf_token);
                                button class="rounded-lg bg-brand px-3.5 py-2 text-[0.78rem] font-semibold text-white transition hover:bg-[#0f4130]" type="submit" {
                                    i class="fa-solid fa-rocket mr-2" aria-hidden="true" {}
                                    "Deploy"
                                }
                            }
                            @if let Some(previous) = previous {
                                form method="post" action=(format!("/environments/{}/rollback", environment.id)) class="mt-2" {
                                    input type="hidden" name="csrf_token" value=(csrf_token);
                                    button class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-ink" type="submit" {
                                        i class="fa-solid fa-rotate-left mr-2" aria-hidden="true" {}
                                        "Rollback to v" (previous.version)
                                    }
                                }
                            }
                            p class="mb-0 mt-3 text-xs text-muted" { "Deployments run through the build runner; watch progress under Deployment." }
                        }
                    }
                    section class="mt-10" {
                        div class="mb-3 flex flex-col gap-1 sm:flex-row sm:items-end sm:justify-between" {
                            h2 class="m-0 text-[1.2rem] font-bold" { "Deployment history" }
                            a class="text-[0.78rem] font-semibold text-brand no-underline hover:underline" href=(format!("/environments/{}/logs", environment.id)) { "Latest logs →" }
                        }
                        @if deployments.is_empty() {
                            p class="rounded-[0.85rem] border border-dashed border-line p-6 text-sm text-muted" { "No deployments yet." }
                        } @else {
                            div class="overflow-x-auto rounded-[0.85rem] border border-line" {
                                table class="w-full border-collapse text-left text-sm" {
                                    thead class="bg-surface/60 text-[0.65rem] uppercase tracking-[0.12em] text-muted" {
                                        tr {
                                            th class="px-4 py-3 font-semibold" { "Version" }
                                            th class="px-4 py-3 font-semibold" { "Status" }
                                            th class="px-4 py-3 font-semibold" { "Trigger" }
                                            th class="px-4 py-3 font-semibold" { "Started" }
                                            th class="px-4 py-3 font-semibold" { "Completed" }
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
                                                td class="px-4 py-3 text-xs text-muted" { (deployment.started_at) }
                                                td class="px-4 py-3 text-xs text-muted" { (deployment.completed_at.as_deref().unwrap_or("—")) }
                                            }
                                        }
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

/// Status strip shared by the detail page and the 5s HTMX partial.
pub fn environment_status_body(environment: &Environment, latest: Option<&Deployment>) -> Markup {
    html! {
        section id="environment-status" hx-get=(format!("/environments/{}/status", environment.id)) hx-trigger="every 5s" hx-swap="outerHTML" {
            div class="mt-6 flex flex-wrap items-center gap-3 rounded-[0.85rem] border border-line bg-surface/45 px-5 py-4" {
                span class=(format!("rounded-full px-3 py-1 text-xs font-semibold {}", environment_status_class(&environment.status))) {
                    (environment.status)
                }
                @match latest {
                    Some(deployment) => {
                        span class="text-sm text-muted" {
                            "Latest deployment: v" (deployment.version) " · "
                            span class=(format!("font-semibold {}", "")) { (deployment.status) }
                            " · " (deployment.trigger_type)
                        }
                    },
                    None => span class="text-sm text-muted" { "No deployments yet." },
                }
            }
        }
    }
}

pub fn environment_deployments(
    base_url: &str,
    username: &str,
    csrf_token: &str,
    environment: &Environment,
    deployments: &[Deployment],
) -> Markup {
    page(
        "Deployments · Hakobi",
        html! {
            div class="min-h-screen lg:pl-60" {
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
                    p class=(format!("{EYEBROW} mb-3 mt-6 text-brand-bright")) { "History" }
                    h1 class="m-0 font-display text-[clamp(2.2rem,5vw,4rem)] font-normal tracking-[-0.04em]" { "Deployments" }
                    @if deployments.is_empty() {
                        div class="mt-8 rounded-[0.85rem] border border-dashed border-line p-8 text-center text-muted" {
                            p class="m-0" { "No deployments recorded for this environment." }
                        }
                    } @else {
                        div class="mt-8 overflow-x-auto rounded-[0.85rem] border border-line" {
                            table class="w-full border-collapse text-left text-sm" {
                                thead class="bg-surface/60 text-[0.65rem] uppercase tracking-[0.12em] text-muted" {
                                    tr {
                                        th class="px-4 py-3 font-semibold" { "Version" }
                                        th class="px-4 py-3 font-semibold" { "Status" }
                                        th class="px-4 py-3 font-semibold" { "Trigger" }
                                        th class="px-4 py-3 font-semibold" { "Commit" }
                                        th class="px-4 py-3 font-semibold" { "Started" }
                                        th class="px-4 py-3 font-semibold" { "Completed" }
                                    }
                                }
                                tbody {
                                    @for deployment in deployments {
                                        tr class="border-t border-line" {
                                            td class="px-4 py-3 font-semibold" { "v" (deployment.version) }
                                            td class="px-4 py-3" {
                                                span class=(format!("rounded-full px-2.5 py-1 text-[0.68rem] font-semibold {}", deployment_status_class(&deployment.status))) { (deployment.status) }
                                            }
                                            td class="px-4 py-3 text-muted" { (deployment.trigger_type) }
                                            td class="px-4 py-3 font-mono text-xs text-muted" { (deployment.git_commit_sha.as_deref().unwrap_or("—")) }
                                            td class="px-4 py-3 text-xs text-muted" { (deployment.started_at) }
                                            td class="px-4 py-3 text-xs text-muted" { (deployment.completed_at.as_deref().unwrap_or("—")) }
                                        }
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

pub fn environment_logs(
    base_url: &str,
    username: &str,
    csrf_token: &str,
    environment: &Environment,
    deployment: Option<&Deployment>,
    logs: &[DeploymentLog],
) -> Markup {
    page(
        "Deployment logs · Hakobi",
        html! {
            div class="min-h-screen lg:pl-60" {
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
                    p class=(format!("{EYEBROW} mb-3 mt-6 text-brand-bright")) { "Output" }
                    h1 class="m-0 font-display text-[clamp(2.2rem,5vw,4rem)] font-normal tracking-[-0.04em]" { "Deployment logs" }
                    @match deployment {
                        Some(deployment) => {
                            p class="mt-3 text-sm text-muted" { "Latest: v" (deployment.version) " · " (deployment.status) }
                        },
                        None => p class="mt-3 text-sm text-muted" { "No deployments yet." },
                    }
                    div class="mt-8 max-h-[36rem] overflow-auto rounded-[0.85rem] border border-line bg-surface p-5 font-mono text-xs" {
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
