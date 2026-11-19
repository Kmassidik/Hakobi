use maud::{html, Markup};

use hakobi::catalog::HostAgent;

use super::{AppFormValues, CONTROL};

pub(super) fn github_source_fields(values: &AppFormValues) -> Markup {
    html! {
        (source_cards(values))
        (connected_source_fields(values))
        (public_source_fields(values))
    }
}

fn source_cards(values: &AppFormValues) -> Markup {
    let public = values.is_public_source();
    html! {
        fieldset class="m-0 min-w-0 border-0 p-0" {
            legend class="px-0 text-[13px] font-semibold text-zinc-800" { "Repository source" }
            div class="mt-3 grid grid-cols-1 gap-3 sm:grid-cols-2" {
                (source_card("connected", "Your repositories", "Pick from repositories available to your connected GitHub App.", !public))
                (source_card("public", "Public repository", "Clone a public repository with an HTTPS URL; no GitHub connection is needed.", public))
            }
        }
    }
}

fn source_card(kind: &str, title: &str, description: &str, selected: bool) -> Markup {
    html! {
        label data-source-card=(kind) data-state=(if selected { "selected" } else { "unselected" }) class=(source_card_class(selected)) {
            input type="radio" name="source_kind" value=(kind) data-source-radio checked[selected] class="mt-0.5 size-4 shrink-0 accent-[#165237]";
            span {
                span class="block text-[12.5px] font-semibold text-zinc-900" { (title) }
                span class="mt-1 block text-[11px] leading-relaxed text-zinc-500" { (description) }
            }
        }
    }
}

fn source_card_class(selected: bool) -> &'static str {
    if selected {
        "flex cursor-pointer items-start gap-3 rounded-xl border border-[#165237] bg-[#165237]/5 p-4 transition"
    } else {
        "flex cursor-pointer items-start gap-3 rounded-xl border border-zinc-200 bg-white p-4 transition"
    }
}

fn connected_source_fields(values: &AppFormValues) -> Markup {
    let public = values.is_public_source();
    html! {
        div data-source-panel="connected" hidden[public] {
            div id="gh-source-picker" data-gh-picker class="grid gap-5" {
                div class="grid grid-cols-1 gap-4 sm:grid-cols-2" {
                    label class="grid gap-2 text-[13px] font-semibold text-zinc-800" {
                        "Repository"
                        select name="github_repo" data-gh-repos class=(CONTROL) disabled[public] {
                            option value="" { "Loading repositories…" }
                            @if !public && !values.github_repo.is_empty() {
                                option value=(values.github_repo.as_str()) selected { (values.github_repo) }
                            }
                        }
                    }
                    label class="grid gap-2 text-[13px] font-semibold text-zinc-800" {
                        "Branch"
                        select name="github_branch" data-gh-branches class=(CONTROL) disabled[public] {
                            @if !public && !values.github_branch.is_empty() {
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
                        "Up to three applications appear as rows; larger repositories use search."
                    }
                    div data-gh-apps class="grid grid-cols-1 gap-3" {}
                }
            }
            p data-gh-status class="m-0 mt-3 text-[11.5px] leading-relaxed text-zinc-500" { "Checking GitHub connection…" }
        }
    }
}

fn public_source_fields(values: &AppFormValues) -> Markup {
    let public = values.is_public_source();
    let branch = if public {
        values.github_branch.as_str()
    } else {
        "main"
    };
    html! {
        div data-source-panel="public" hidden[!public] class="grid grid-cols-1 gap-4 sm:grid-cols-2" {
            label class="grid gap-2 text-[13px] font-semibold text-zinc-800 sm:col-span-2" {
                "Repository URL"
                input class=(CONTROL) name="github_repo" type="url" pattern="https://.*" value=(if public { values.github_repo.as_str() } else { "" }) maxlength="2048" required[public] disabled[!public] autocomplete="off" spellcheck="false" placeholder="https://github.com/owner/repository.git";
                span class="text-[11.5px] font-normal leading-relaxed text-zinc-500" {
                    "HTTPS only. Credentials, query parameters, and fragments are not allowed."
                }
            }
            label class="grid gap-2 text-[13px] font-semibold text-zinc-800" {
                "Branch or tag"
                input class=(CONTROL) name="github_branch" type="text" value=(branch) maxlength="255" required[public] disabled[!public] autocomplete="off" placeholder="main";
                span class="text-[11.5px] font-normal leading-relaxed text-zinc-500" {
                    "Hakobi clones this ref for each manual deployment."
                }
            }
        }
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
        label class="grid gap-2 text-[13px] font-semibold text-zinc-800" {
            "Build type"
            select class=(CONTROL) name="build_type" required {
                option value="dockerfile" selected[values.build_type == "dockerfile"] { "Dockerfile" }
                option value="nixpacks" selected[values.build_type == "nixpacks"] { "Nixpacks" }
            }
        }
        fieldset class="m-0 min-w-0 border-0 border-t border-zinc-200 p-0 pt-5" {
            legend class="px-0 text-[13px] font-semibold text-zinc-800" { "Port mapping" }
            p class="mb-4 mt-1 text-[11.5px] leading-relaxed text-zinc-500" {
                "Map an automatic or custom loopback host port to the application’s container port."
            }
            div class="grid grid-cols-1 gap-4 sm:grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)] sm:items-center" {
                label class="grid gap-2 text-[13px] font-semibold text-zinc-800" {
                    "Host port (optional)"
                    input class=(CONTROL) name="requested_host_port" type="number" value=(values.requested_host_port) min="1" max="65535" placeholder="Automatic" autocomplete="off";
                    span class="text-[11.5px] font-normal leading-relaxed text-zinc-500" {
                        "Blank selects an available loopback port."
                    }
                }
                span class="hidden pt-5 text-zinc-400 sm:block" aria-hidden="true" { "→" }
                label class="grid gap-2 text-[13px] font-semibold text-zinc-800" {
                    "Container port"
                    input class=(CONTROL) name="container_port" type="number" value=(values.container_port) min="1" max="65535" placeholder="8080";
                    span class="text-[11.5px] font-normal leading-relaxed text-zinc-500" {
                        "The port your application listens on."
                    }
                }
            }
        }
        (environment_fields(values))
        (super::runtime::runtime_fields(values))
        section class="grid gap-5 border-t border-zinc-200 pt-5" aria-labelledby="build-paths-title" {
            h3 id="build-paths-title" class="m-0 text-[13px] font-semibold text-zinc-800" { "Build paths" }
            label class="grid gap-2 text-[13px] font-semibold text-zinc-800" {
                "Context path"
                input class=(CONTROL) name="github_path" type="text" value=(values.github_path) maxlength="512" placeholder="." autocomplete="off";
                span class="text-[11.5px] font-normal leading-relaxed text-zinc-500" { "Repository subdirectory, or blank for the root." }
            }
            label class="grid gap-2 text-[13px] font-semibold text-zinc-800" {
                "Dockerfile path"
                input class=(CONTROL) name="github_dockerfile" type="text" value=(values.github_dockerfile) maxlength="512" placeholder="Dockerfile" autocomplete="off";
                span class="text-[11.5px] font-normal leading-relaxed text-zinc-500" { "Relative path to the Dockerfile used for the image build." }
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
                div class="hidden sm:grid sm:grid-cols-12 sm:gap-3 text-[11px] font-semibold text-zinc-500" {
                    span class="sm:col-span-3" { "Type" }
                    span class="sm:col-span-4" { "Name" }
                    span class="sm:col-span-4" { "Value" }
                    span class="sm:col-span-1" {}
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
        div data-env-row class="grid grid-cols-1 gap-3 rounded-lg border border-zinc-200 bg-zinc-50/60 p-3 sm:grid-cols-12 sm:items-center sm:rounded-none sm:border-0 sm:bg-transparent sm:p-0" {
            label class="grid gap-1.5 text-[11px] font-semibold text-zinc-500 sm:col-span-3" {
                span class="sm:hidden" { "Type" }
                select data-env-type class=(CONTROL) aria-label="Variable type" {
                    option value="variable" { "Variable" }
                }
            }
            label class="grid gap-1.5 text-[11px] font-semibold text-zinc-500 sm:col-span-4" {
                span class="sm:hidden" { "Name" }
                input data-env-key class=(CONTROL) type="text" placeholder="Name" autocomplete="off";
            }
            label class="grid gap-1.5 text-[11px] font-semibold text-zinc-500 sm:col-span-4" {
                span class="sm:hidden" { "Value" }
                input data-env-value class=(CONTROL) type="text" placeholder="Value" autocomplete="off";
            }
            button type="button" data-env-remove class="inline-flex min-h-10 items-center justify-center gap-2 rounded-lg border border-danger/30 bg-white px-3 text-danger hover:bg-danger/5 sm:col-span-1 sm:w-full sm:px-0" aria-label="Remove variable" title="Remove variable" {
                i class="fa-solid fa-trash-can" aria-hidden="true" {}
                span class="sm:hidden" { "Remove" }
            }
        }
    }
}
