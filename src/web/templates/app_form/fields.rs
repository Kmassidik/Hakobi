use maud::{html, Markup};

use hakobi::catalog::HostAgent;

use super::{AppFormValues, CONTROL};

pub(super) fn github_source_fields(values: &AppFormValues) -> Markup {
    html! {
        div id="gh-source-picker" data-gh-picker class="grid gap-5" {
            div class="grid grid-cols-1 gap-4 sm:grid-cols-2" {
                label class="grid gap-2 text-[13px] font-semibold text-zinc-800" {
                    "Repository"
                    select name="github_repo" data-gh-repos class=(CONTROL) {
                        option value="" { "Loading repositories…" }
                        @if !values.github_repo.is_empty() {
                            option value=(values.github_repo.as_str()) selected { (values.github_repo) }
                        }
                    }
                }
                label class="grid gap-2 text-[13px] font-semibold text-zinc-800" {
                    "Branch"
                    select name="github_branch" data-gh-branches class=(CONTROL) {
                        @if !values.github_branch.is_empty() {
                            option value=(values.github_branch.as_str()) selected { (values.github_branch) }
                        } @else {
                            option value="main" { "main" }
                        }
                    }
                }
            }
            fieldset class="m-0 min-w-0 border-0 p-0" {
                legend class="px-0 text-[13px] font-semibold text-zinc-800" { "Detected applications" }
                p class="mb-3 mt-1 text-[11.5px] leading-relaxed text-zinc-500" {
                    "Select the repository root or a detected subdirectory to build."
                }
                div data-gh-apps class="grid grid-cols-1 gap-3" {}
            }
        }
        p data-gh-status class="m-0 text-[11.5px] leading-relaxed text-zinc-500" { "Checking GitHub connection…" }
    }
}

pub(super) fn app_identity_fields(hosts: &[HostAgent], values: &AppFormValues) -> Markup {
    html! {
        label class="grid gap-2 text-[13px] font-semibold text-zinc-800" {
            "Name"
            input class=(CONTROL) name="name" type="text" value=(values.name) maxlength="64" pattern="[A-Za-z0-9][A-Za-z0-9._-]*" required autocomplete="off" placeholder="Application name";
            span class="text-[11.5px] font-normal leading-relaxed text-zinc-500" {
                "1–64 ASCII letters, digits, dots, dashes, or underscores. Must be unique."
            }
        }
        @if hosts.len() == 1 {
            input type="hidden" name="host_id" value=(hosts[0].id) data-summary-label=(format!("{} · {}", hosts[0].name, hosts[0].agent_url));
        } @else {
            label class="grid gap-2 text-[13px] font-semibold text-zinc-800" {
                "Host"
                select class=(CONTROL) name="host_id" required {
                    option value="" disabled selected[values.host_id.is_empty()] { "Select an enabled host…" }
                    @for host in hosts {
                        option value=(host.id) selected[values.host_id == host.id] {
                            (host.name) " · " (host.agent_url)
                        }
                    }
                }
                span class="text-[11.5px] font-normal leading-relaxed text-zinc-500" {
                    "Only enrolled, enabled host agents are available."
                }
            }
        }
    }
}

pub(super) fn build_settings_fields(values: &AppFormValues) -> Markup {
    html! {
        div class="grid grid-cols-1 gap-4 sm:grid-cols-3" {
            label class="grid gap-2 text-[13px] font-semibold text-zinc-800" {
                "Build type"
                select class=(CONTROL) name="build_type" required {
                    option value="dockerfile" selected[values.build_type == "dockerfile"] { "Dockerfile" }
                    option value="nixpacks" selected[values.build_type == "nixpacks"] { "Nixpacks" }
                }
            }
            label class="grid gap-2 text-[13px] font-semibold text-zinc-800" {
                "Host port (optional)"
                input class=(CONTROL) name="requested_host_port" type="number" value=(values.requested_host_port) min="1" max="65535" placeholder="Automatic" autocomplete="off";
                span class="text-[11.5px] font-normal leading-relaxed text-zinc-500" {
                    "Loopback-only. Leave blank to automatically assign a host port."
                }
            }
            label class="grid gap-2 text-[13px] font-semibold text-zinc-800" {
                "Container port"
                input class=(CONTROL) name="container_port" type="number" value=(values.container_port) min="1" max="65535" placeholder="8080";
            }
        }
        (environment_fields(values))
        div class="grid grid-cols-1 gap-4 sm:grid-cols-2" {
            label class="grid gap-2 text-[13px] font-semibold text-zinc-800" {
                "Context path"
                input class=(CONTROL) name="github_path" type="text" value=(values.github_path) maxlength="512" placeholder="." autocomplete="off";
                span class="text-[11.5px] font-normal leading-relaxed text-zinc-500" { "Repository subdirectory, or blank for the root." }
            }
            label class="grid gap-2 text-[13px] font-semibold text-zinc-800" {
                "Dockerfile path"
                input class=(CONTROL) name="github_dockerfile" type="text" value=(values.github_dockerfile) maxlength="512" placeholder="Dockerfile" autocomplete="off";
            }
        }
    }
}

fn environment_fields(values: &AppFormValues) -> Markup {
    html! {
        section class="grid gap-4 border-y border-zinc-200 py-5" aria-labelledby="environment-title" {
            label data-env-raw class="grid gap-2 text-[13px] font-semibold text-zinc-800" {
                "Environment variables (JSON, optional)"
                textarea class=(CONTROL) name="env_json" data-env-json rows="4" placeholder="{}" autocomplete="off" { (values.env_json) }
            }
            div data-env-editor hidden class="grid gap-4" {
                div class="flex items-start justify-between gap-4" {
                    div {
                        h3 id="environment-title" class="m-0 text-[13px] font-semibold text-zinc-800" { "Environment variables" }
                        p class="mb-0 mt-1 text-[11.5px] font-normal leading-relaxed text-zinc-500" {
                            "Add plain-text variables for the container."
                        }
                    }
                    button type="button" data-env-add class="inline-flex min-h-9 shrink-0 items-center gap-1.5 rounded-lg border border-zinc-200 bg-white px-3 py-1.5 text-[12px] font-semibold text-zinc-700 hover:bg-zinc-50" {
                        i class="fa-solid fa-plus text-[9px]" aria-hidden="true" {}
                        "Add variable"
                    }
                }
                div data-env-rows class="grid gap-3" {}
                template data-env-row-template { (environment_row()) }
                p data-env-error role="alert" class="m-0 text-[11.5px] font-normal text-danger" {}
            }
            span data-gh-env-status class="text-[11.5px] font-normal leading-relaxed text-zinc-500" {
                "After scanning, keys from .env.example or .env.sample appear here with blank values."
            }
        }
    }
}

fn environment_row() -> Markup {
    html! {
        div data-env-row class="grid grid-cols-1 gap-3 rounded-lg border border-zinc-200 bg-zinc-50/60 p-3 sm:grid-cols-12 sm:items-end sm:rounded-none sm:border-0 sm:bg-transparent sm:p-0" {
            label class="grid gap-1.5 text-[11px] font-semibold text-zinc-500 sm:col-span-3" {
                "Type"
                select data-env-type class=(CONTROL) aria-label="Variable type" {
                    option value="variable" { "Variable" }
                }
            }
            label class="grid gap-1.5 text-[11px] font-semibold text-zinc-500 sm:col-span-4" {
                "Name"
                input data-env-key class=(CONTROL) type="text" placeholder="Name" autocomplete="off";
            }
            label class="grid gap-1.5 text-[11px] font-semibold text-zinc-500 sm:col-span-4" {
                "Value"
                input data-env-value class=(CONTROL) type="text" placeholder="Value" autocomplete="off";
            }
            button type="button" data-env-remove class="inline-flex min-h-10 items-center justify-center gap-2 rounded-lg border border-danger/30 bg-white px-3 text-danger hover:bg-danger/5 sm:col-span-1 sm:w-full sm:px-0" aria-label="Remove variable" title="Remove variable" {
                i class="fa-solid fa-trash-can" aria-hidden="true" {}
                span class="sm:hidden" { "Remove" }
            }
        }
    }
}
