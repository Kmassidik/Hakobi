//! App create/edit form template and delete confirmation dialog.

use maud::{html, Markup};

use hakobi::catalog::HostAgent;

use super::{brand, page, sidebar, CONTENT_WIDTH, EYEBROW, FIELD};

pub struct AppFormValues {
    pub name: String,
    pub host_id: String,
    pub build_type: String,
    pub github_repo: String,
    pub github_branch: String,
    pub github_path: String,
    pub github_dockerfile: String,
    pub github_auto_deploy: bool,
    pub container_port: String,
}

#[allow(clippy::too_many_arguments)]
pub fn app_form(
    base_url: &str,
    username: &str,
    csrf_token: &str,
    hosts: &[HostAgent],
    values: &AppFormValues,
    action: &str,
    heading: &str,
    submit_label: &str,
    error: Option<&str>,
) -> Markup {
    page(
        "App form · Hakobi",
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
                    p class=(format!("{EYEBROW} mb-3 mt-6 text-brand-bright")) { "App" }
                    h1 class="m-0 font-display text-[clamp(2.2rem,5vw,4rem)] font-normal tracking-[-0.04em]" { (heading) }
                    p class="mt-4 max-w-[44rem] leading-[1.7] text-muted" {
                        "Bind a build recipe to an enrolled host. On push to the tracked branch, Hakobi rebuilds and redeploys automatically."
                    }
                    @if let Some(message) = error {
                        div class="mb-6 mt-6 border-l-[3px] border-danger bg-danger/5 px-4 py-3.5 text-sm leading-normal text-danger" role="alert" { (message) }
                    }
                    form method="post" action=(action) class="mt-8 grid max-w-[44rem] grid-cols-1 gap-5 rounded-[0.85rem] border border-line bg-surface/45 p-6" {
                        input type="hidden" name="csrf_token" value=(csrf_token);
                        p class=(format!("{EYEBROW} mb-1 mt-0 text-brand-bright")) { "GitHub source" }
                        p class="m-0 text-sm leading-relaxed text-muted" {
                            "Bind this app to a repository. On push, Hakobi rebuilds and redeploys automatically."
                        }
                        div id="gh-source-picker" data-gh-picker class="mt-4 grid grid-cols-1 gap-4" {
                            label class="grid gap-1.5" {
                                span class="text-[0.7rem] font-bold uppercase tracking-[0.08em] text-[#86868b]" { "Repository" }
                                select name="github_repo" data-gh-repos class="rounded-lg border border-line bg-surface px-3 py-2.5 text-sm" {
                                    option value="" { "Loading repositories…" }
                                }
                            }
                            label class="grid gap-1.5" {
                                span class="text-[0.7rem] font-bold uppercase tracking-[0.08em] text-[#86868b]" { "Branch" }
                                select name="github_branch" data-gh-branches class="rounded-lg border border-line bg-surface px-3 py-2.5 text-sm" {
                                    option value="main" { "main" }
                                }
                            }
                            div data-gh-apps class="grid grid-cols-1 gap-3" {}
                            label class="grid gap-1.5" {
                                span class="text-[0.7rem] font-bold uppercase tracking-[0.08em] text-[#86868b]" { "Auto-deploy on push" }
                                input name="github_auto_deploy" type="checkbox" value="1" checked[values.github_auto_deploy] class="size-4 justify-self-start";
                            }
                        }
                        p data-gh-status class="m-0 text-xs text-muted" { "Checking GitHub connection…" }
                        label class="flex flex-col gap-2 text-[0.8rem] font-bold" {
                            "Name"
                            p class="m-0 text-xs font-normal leading-relaxed text-muted" {
                                "1–64 ASCII letters, digits, dots, dashes, or underscores. Must be unique."
                            }
                            input class=(FIELD) name="name" type="text" value=(values.name) maxlength="64" pattern="[A-Za-z0-9][A-Za-z0-9._-]*" required autocomplete="off";
                        }
                        label class="flex flex-col gap-2 text-[0.8rem] font-bold" {
                            "Host"
                            p class="m-0 text-xs font-normal leading-relaxed text-muted" {
                                "Enrolled host agents only. Disabled hosts are hidden."
                            }
                            select class=(FIELD) name="host_id" required {
                                option value="" disabled selected[values.host_id.is_empty()] { "Select an enabled host…" }
                                @for host in hosts {
                                    option value=(host.id) selected[values.host_id == host.id] {
                                        (host.name) " · " (host.agent_url)
                                    }
                                }
                            }
                        }
                        div class="grid grid-cols-1 gap-5 md:grid-cols-2" {
                            label class="flex flex-col gap-2 text-[0.8rem] font-bold" {
                                "Build type"
                                select class=(FIELD) name="build_type" required {
                                    @for build in ["dockerfile", "nixpacks"] {
                                        option value=(build) selected[values.build_type == build] { (build) }
                                    }
                                }
                            }
                            label class="flex flex-col gap-2 text-[0.8rem] font-bold" {
                                "Container port"
                                input class=(FIELD) name="container_port" type="number" value=(values.container_port) min="1" max="65535" placeholder="8080";
                            }
                        }
                        label class="flex flex-col gap-2 text-[0.8rem] font-bold" {
                            "Context path"
                            p class="m-0 text-xs font-normal leading-relaxed text-muted" {
                                "Subdirectory of the repository to build, e.g. apps/web. Empty for the repo root."
                            }
                            input class=(FIELD) name="github_path" type="text" value=(values.github_path) maxlength="512" placeholder="." autocomplete="off";
                        }
                        label class="flex flex-col gap-2 text-[0.8rem] font-bold" {
                            "Dockerfile path"
                            input class=(FIELD) name="github_dockerfile" type="text" value=(values.github_dockerfile) maxlength="512" placeholder="Dockerfile" autocomplete="off";
                        }
                        div class="flex flex-wrap items-center gap-3" {
                            button class="rounded-[0.6rem] bg-brand px-4 py-3 font-bold text-white transition hover:bg-[#0f4130]" type="submit" { (submit_label) }
                            a class="text-[0.8rem] font-semibold text-muted no-underline hover:text-ink" href="/apps" { "Cancel" }
                        }
                    }
                }
            }
        },
    )
}

pub(crate) fn delete_app_dialog(app_id: &str, csrf_token: &str) -> Markup {
    html! {
        dialog id="delete-app" class="w-[90%] max-w-[44rem] rounded-[0.85rem] border border-line bg-surface p-6 text-ink" aria-labelledby="delete-app-title" {
            form method="post" action=(format!("/apps/{app_id}/delete")) class="m-0" {
                input type="hidden" name="csrf_token" value=(csrf_token);
                h2 id="delete-app-title" class="m-0 font-display text-[1.5rem] font-normal" { "Delete app?" }
                p class="mt-3 text-sm leading-relaxed text-muted" {
                    "This permanently removes " strong class="text-ink" data-confirm-name { "this app" } ", its deployments, and their logs."
                }
                label class="mt-4 flex flex-col gap-2 text-[0.8rem] font-bold" {
                    "Type the app name to confirm"
                    input class=(FIELD) name="confirm_name" type="text" autocomplete="off" data-confirm-input aria-describedby="delete-app-help" required;
                }
                p id="delete-app-help" class="mb-0 mt-2 text-xs text-muted" { "The name must match exactly." }
                div class="mt-5 flex flex-wrap gap-2" {
                    button type="button" class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-ink" data-dialog-close { "Cancel" }
                    button type="submit" class="rounded-lg border border-danger/40 bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-danger" data-confirm-submit disabled { "Delete app" }
                }
            }
        }
    }
}
