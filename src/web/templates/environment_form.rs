//! Environment create/edit form template and delete confirmation dialog.

use maud::{html, Markup};

use hakobi::catalog::HostAgent;

use super::{brand, page, sidebar, CONTENT_WIDTH, EYEBROW, FIELD};

/// Prefilled values for the environment create and edit forms.
pub struct EnvironmentFormValues {
    pub name: String,
    pub host_id: String,
    pub build_type: String,
    pub source_type: String,
    pub git_url: String,
    pub git_branch: String,
    pub registry_mode: String,
}

#[allow(clippy::too_many_arguments)]
pub fn environment_form(
    base_url: &str,
    username: &str,
    csrf_token: &str,
    hosts: &[HostAgent],
    values: &EnvironmentFormValues,
    action: &str,
    heading: &str,
    submit_label: &str,
    error: Option<&str>,
) -> Markup {
    page(
        "Environment form · Hakobi",
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
                    a class="text-[0.75rem] font-semibold text-muted no-underline hover:text-ink" href="/environments" { "← All environments" }
                    p class=(format!("{EYEBROW} mb-3 mt-6 text-brand-bright")) { "Environment" }
                    h1 class="m-0 font-display text-[clamp(2.2rem,5vw,4rem)] font-normal tracking-[-0.04em]" { (heading) }
                    p class="mt-4 max-w-[44rem] leading-[1.7] text-muted" {
                        "Bind a build recipe to an enrolled host. The source decides whether Hakobi clones a repository or reads an uploaded tarball."
                    }
                    @if let Some(message) = error {
                        div class="mb-6 mt-6 border-l-[3px] border-danger bg-danger/5 px-4 py-3.5 text-sm leading-normal text-danger" role="alert" { (message) }
                    }
                    form method="post" action=(action) class="mt-8 grid max-w-[44rem] grid-cols-1 gap-5 rounded-[0.85rem] border border-line bg-surface/45 p-6" {
                        input type="hidden" name="csrf_token" value=(csrf_token);
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
                                    @for build in ["compose", "dockerfile", "nixpacks"] {
                                        option value=(build) selected[values.build_type == build] { (build) }
                                    }
                                }
                            }
                            label class="flex flex-col gap-2 text-[0.8rem] font-bold" {
                                "Source type"
                                select class=(FIELD) name="source_type" required {
                                    @for source in ["git", "upload"] {
                                        option value=(source) selected[values.source_type == source] { (source) }
                                    }
                                }
                            }
                        }
                        label class="flex flex-col gap-2 text-[0.8rem] font-bold" {
                            "Git URL"
                            p class="m-0 text-xs font-normal leading-relaxed text-muted" {
                                "Required for git source, empty for upload source."
                            }
                            input class=(FIELD) name="git_url" type="text" value=(values.git_url) maxlength="2048" placeholder="https://github.com/user/repo.git" autocomplete="off";
                        }
                        label class="flex flex-col gap-2 text-[0.8rem] font-bold" {
                            "Branch"
                            input class=(FIELD) name="git_branch" type="text" value=(values.git_branch) maxlength="256" placeholder="main" autocomplete="off";
                        }
                        label class="flex flex-col gap-2 text-[0.8rem] font-bold" {
                            "Registry mode"
                            select class=(FIELD) name="registry_mode" required {
                                option value="local" selected[values.registry_mode == "local"] { "local — images stay on the host" }
                                option value="push" selected[values.registry_mode == "push"] { "push — images go to an external registry" }
                            }
                            span class="mt-1 inline-flex w-fit items-center gap-1 rounded-full bg-brand-bright/10 px-2.5 py-1 text-[0.68rem] font-semibold text-brand" {
                                "local keeps images on the host; push sends them to an external registry"
                            }
                        }
                        div class="flex flex-wrap items-center gap-3" {
                            button class="rounded-[0.6rem] bg-brand px-4 py-3 font-bold text-white transition hover:bg-[#0f4130]" type="submit" { (submit_label) }
                            a class="text-[0.8rem] font-semibold text-muted no-underline hover:text-ink" href="/environments" { "Cancel" }
                        }
                    }
                }
            }
        },
    )
}

pub(crate) fn delete_environment_dialog(csrf_token: &str) -> Markup {
    html! {
        dialog id="delete-environment" class="w-[90%] max-w-[44rem] rounded-[0.85rem] border border-line bg-surface p-6 text-ink" aria-labelledby="delete-environment-title" {
            form method="post" action="/environments" class="m-0" {
                input type="hidden" name="csrf_token" value=(csrf_token);
                h2 id="delete-environment-title" class="m-0 font-display text-[1.5rem] font-normal" { "Delete environment?" }
                p class="mt-3 text-sm leading-relaxed text-muted" {
                    "This permanently removes " strong class="text-ink" data-confirm-name { "this environment" } ", its deployments, and their logs."
                }
                label class="mt-4 flex flex-col gap-2 text-[0.8rem] font-bold" {
                    "Type the environment name to confirm"
                    input class=(FIELD) name="confirm_name" type="text" autocomplete="off" data-confirm-input aria-describedby="delete-environment-help" required;
                }
                p id="delete-environment-help" class="mb-0 mt-2 text-xs text-muted" { "The name must match exactly." }
                div class="mt-5 flex flex-wrap gap-2" {
                    button type="button" class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-ink" data-dialog-close { "Cancel" }
                    button type="submit" class="rounded-lg border border-danger/40 bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-danger" data-confirm-submit disabled { "Delete environment" }
                }
            }
        }
    }
}
