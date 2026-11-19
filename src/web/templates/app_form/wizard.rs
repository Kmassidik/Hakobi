use maud::{html, Markup};

use hakobi::catalog::HostAgent;

use super::{AppFormValues, CONTROL};

pub(super) fn stepper_navigation() -> Markup {
    html! {
        ol hidden data-step-progress class="m-0 grid list-none grid-cols-2 gap-2 rounded-xl border border-zinc-200 bg-white p-3 shadow-xs sm:grid-cols-4 sm:gap-3 sm:p-4" aria-label="App creation progress" {
            (stepper_step(1, "Source", "Repository & app"))
            (stepper_step(2, "Identity", "Name & host"))
            (stepper_step(3, "Build", "Runtime & variables"))
            (stepper_step(4, "Review", "Resources & deploy"))
        }
    }
}

fn stepper_step(number: u8, title: &str, subtitle: &str) -> Markup {
    let state = if number == 1 { "active" } else { "upcoming" };
    html! {
        li class="min-w-0" {
            button type="button" data-step-target=(number) data-step-state=(state) data-state=(state) aria-current=(if number == 1 { "step" } else { "false" }) class="w-full rounded-lg border border-transparent bg-white p-2.5 text-left transition hover:bg-zinc-50 data-[state=active]:border-zinc-200 data-[state=active]:bg-zinc-50" {
                span class="flex items-start gap-3" {
                    span data-step-badge data-step-state=(state) data-state=(state) class="mt-0.5 grid size-6 shrink-0 place-items-center rounded-full border border-zinc-200 bg-white text-[11px] font-semibold text-zinc-500 data-[state=active]:border-[#165237] data-[state=active]:bg-[#165237] data-[state=active]:text-white data-[state=completed]:border-[#165237]/30 data-[state=completed]:bg-emerald-50 data-[state=completed]:text-[#165237]" { (number) }
                    span class="min-w-0" {
                        span data-step-title data-step-state=(state) data-state=(state) class="block text-[12.5px] font-semibold text-zinc-500 data-[state=active]:text-[#165237] data-[state=completed]:text-zinc-900" { (title) }
                        span class="mt-0.5 block truncate text-[10.5px] text-zinc-400" { (subtitle) }
                    }
                }
                span data-step-bar data-step-state=(state) data-state=(state) class="mt-2 block h-1 w-full rounded-full bg-zinc-100 data-[state=active]:bg-[#165237] data-[state=completed]:bg-[#165237]/60" {}
            }
        }
    }
}

pub(super) fn recipe_summary(hosts: &[HostAgent], values: &AppFormValues) -> Markup {
    let host = hosts
        .iter()
        .find(|host| host.id == values.host_id)
        .or_else(|| hosts.first().filter(|_| hosts.len() == 1))
        .map(|host| host.name.clone())
        .unwrap_or_else(|| "Not selected".into());
    html! {
        aside class="lg:col-span-4 lg:sticky lg:top-6" aria-labelledby="recipe-summary-title" {
            section class="overflow-hidden rounded-xl border border-zinc-200 bg-white shadow-xs" {
                div class="flex items-center justify-between gap-3 border-b border-zinc-200 px-5 py-4" {
                    div class="flex items-center gap-2" {
                        span class="size-2 rounded-full bg-[#165237]" {}
                        h2 id="recipe-summary-title" class="m-0 text-[11px] font-semibold uppercase tracking-wider text-zinc-700" { "Recipe Summary" }
                    }
                    span data-summary="step" class="rounded bg-zinc-100 px-2 py-0.5 font-mono text-[10.5px] font-medium text-zinc-600" { "1/4" }
                }
                dl class="m-0 divide-y divide-zinc-100 px-5" {
                    (summary_row("App", "name", present(&values.name)))
                    (summary_row("Repository", "repo", present(&values.github_repo)))
                    (summary_row("Branch", "branch", present(&values.github_branch)))
                    (summary_row("Host", "host", host))
                    (summary_row("Runtime", "runtime", format!("{} · {}", build_label(&values.build_type), port_mapping(values))))
                    (summary_row("Allocation", "allocation", format!("{} CPU · {} MiB", present(&values.cpu_limit), present(&values.memory_limit_mb))))
                    (summary_row("Environment", "environment", environment_label(&values.env_json)))
                    (summary_row("Auto-deploy", "auto_deploy", if values.github_auto_deploy { "Auto-deploy on push".into() } else { "Manual deploy only".into() }))
                }
            }
        }
    }
}

fn summary_row(label: &str, key: &str, value: String) -> Markup {
    html! {
        div class="flex items-center justify-between gap-3 py-3 text-[11.5px]" {
            dt class="shrink-0 text-zinc-500" { (label) }
            dd data-summary=(key) class="m-0 min-w-0 break-all text-right font-mono text-[11px] leading-relaxed text-zinc-800" { (value) }
        }
    }
}

pub(super) fn resource_limit_fields(values: &AppFormValues) -> Markup {
    let recommended = values.cpu_limit == "1" && values.memory_limit_mb == "512";
    html! {
        fieldset class="m-0 min-w-0 border-0 p-0" data-resource-settings {
            legend class="px-0 text-[13px] font-semibold text-zinc-800" { "Runtime allocation" }
            p class="mb-4 mt-1 text-[11.5px] font-normal leading-relaxed text-zinc-500" {
                "Start with the recommended allocation or set explicit limits."
            }
            (resource_cards(recommended))
            div class="mt-4 grid grid-cols-1 gap-4 sm:grid-cols-2" {
                label class="grid gap-2 text-[12px] font-semibold text-zinc-700" {
                    "CPU limit (cores)"
                    input class=(CONTROL) name="cpu_limit" type="number" value=(values.cpu_limit) min="0.01" max="256" step="0.01" required data-resource-cpu;
                }
                label class="grid gap-2 text-[12px] font-semibold text-zinc-700" {
                    "Memory limit (MiB)"
                    input class=(CONTROL) name="memory_limit_mb" type="number" value=(values.memory_limit_mb) min="16" max="1048576" step="1" required data-resource-memory;
                }
            }
        }
    }
}

fn resource_cards(recommended: bool) -> Markup {
    html! {
        div class="grid grid-cols-1 gap-3 sm:grid-cols-2" {
            label data-resource-card="recommended" data-state=(if recommended { "selected" } else { "unselected" }) data-selected=(recommended) class=(resource_card_class(recommended)) {
                input type="radio" name="resource_mode" value="recommended" data-resource-recommended checked[recommended] class="mt-0.5 size-4 shrink-0 accent-[#165237]";
                span {
                    span class="block text-[12.5px] font-semibold text-zinc-900" { "Recommended" }
                    span class="mt-1 block text-[11px] leading-relaxed text-zinc-500" { "1 CPU core · 512 MiB" }
                }
            }
            label data-resource-card="custom" data-state=(if recommended { "unselected" } else { "selected" }) data-selected=(!recommended) class=(resource_card_class(!recommended)) {
                input type="radio" name="resource_mode" value="custom" data-resource-custom checked[!recommended] class="mt-0.5 size-4 shrink-0 accent-[#165237]";
                span {
                    span class="block text-[12.5px] font-semibold text-zinc-900" { "Custom" }
                    span class="mt-1 block text-[11px] leading-relaxed text-zinc-500" { "Set your own CPU and memory limits." }
                }
            }
        }
    }
}

fn resource_card_class(selected: bool) -> &'static str {
    if selected {
        "flex cursor-pointer items-start gap-3 rounded-xl border border-[#165237] bg-[#165237]/5 p-4 transition"
    } else {
        "flex cursor-pointer items-start gap-3 rounded-xl border border-zinc-200 bg-white p-4 transition"
    }
}

pub(super) fn deployment_policy_field(values: &AppFormValues) -> Markup {
    let public = values.is_public_source();
    html! {
        div data-connected-deploy-policy hidden[public] {
            label class="flex cursor-pointer items-start gap-3 rounded-xl border border-zinc-200 bg-white p-4" {
                input name="github_auto_deploy" type="checkbox" value="1" checked[values.github_auto_deploy] disabled[public] class="mt-0.5 size-4 shrink-0 accent-[#165237]";
                span {
                    span class="block text-[12.5px] font-semibold text-zinc-900" { "Automatically deploy on push" }
                    span class="mt-1 block text-[11px] leading-relaxed text-zinc-500" {
                        "Hakobi rebuilds and redeploys when the tracked branch changes."
                    }
                }
            }
        }
        div data-public-deploy-policy hidden[!public] class="rounded-xl border border-zinc-200 bg-zinc-50 p-4" {
            span class="block text-[12.5px] font-semibold text-zinc-900" { "Manual deployments only" }
            span class="mt-1 block text-[11px] leading-relaxed text-zinc-500" {
                "Public repositories do not receive GitHub App push events. Use Deploy latest on the app page."
            }
        }
    }
}

pub(super) fn review_summary(values: &AppFormValues) -> Markup {
    let repo = format!(
        "{} · {}",
        present(&values.github_repo),
        present(&values.github_branch)
    );
    let build = format!(
        "{} · {}",
        values.build_type,
        present(&values.github_dockerfile)
    );
    html! {
        section class="overflow-hidden rounded-xl border border-zinc-200" aria-labelledby="app-review-title" {
            div class="border-b border-zinc-200 bg-zinc-50/70 px-4 py-3" {
                h3 id="app-review-title" class="m-0 text-[12.5px] font-semibold text-zinc-900" { "Review" }
                p class="mb-0 mt-1 text-[11px] text-zinc-500" { "Check the recipe before building." }
            }
            dl class="m-0 divide-y divide-zinc-100" data-app-review {
                (review_row("Repository", "repo", repo, Some(1)))
                (review_row("App", "name", present(&values.name), Some(2)))
                (review_row("Build", "build", build, Some(3)))
                (review_row("Port mapping", "port", port_mapping(values), Some(3)))
                (review_row("Environment", "environment", environment_label(&values.env_json), Some(3)))
                (review_row("Secrets", "secrets", count_label(&values.secrets_json, "secret"), Some(3)))
                (review_row("Workers", "processes", count_label(&values.processes_json, "worker"), Some(3)))
                (review_row("Volumes", "volumes", count_label(&values.volumes_json, "volume"), Some(3)))
            }
        }
    }
}

fn review_row(label: &str, key: &str, value: String, target: Option<u8>) -> Markup {
    html! {
        div class="grid grid-cols-1 gap-2 px-4 py-3 sm:grid-cols-[6rem_minmax(0,1fr)_auto] sm:items-center sm:gap-3" {
            dt class="text-[11.5px] text-zinc-500" { (label) }
            dd data-review=(key) class="m-0 break-all font-mono text-[11px] leading-relaxed text-zinc-800" { (value) }
            @if let Some(step) = target {
                button type="button" data-review-target=(step) class="justify-self-start text-[11px] font-semibold text-[#165237] hover:underline sm:justify-self-end" { "Edit" }
            }
        }
    }
}

fn port_mapping(values: &AppFormValues) -> String {
    let host = if values.requested_host_port.trim().is_empty() {
        "auto"
    } else {
        values.requested_host_port.trim()
    };
    format!("{} → {}", host, present(&values.container_port))
}

fn count_label(value: &str, noun: &str) -> String {
    let count = serde_json::from_str::<serde_json::Value>(value)
        .ok()
        .and_then(|value| match value {
            serde_json::Value::Array(values) => Some(values.len()),
            serde_json::Value::Object(values) => Some(values.len()),
            _ => None,
        })
        .unwrap_or(0);
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}

fn present(value: &str) -> String {
    if value.is_empty() {
        "—".into()
    } else {
        value.into()
    }
}

fn build_label(build: &str) -> String {
    match build {
        "dockerfile" => "Dockerfile".into(),
        "nixpacks" => "Nixpacks".into(),
        _ => present(build),
    }
}

fn environment_label(json: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(json) {
        Ok(serde_json::Value::Object(values)) => {
            let count = values.len();
            format!(
                "{count} {}",
                if count == 1 { "variable" } else { "variables" }
            )
        }
        _ => "Invalid variables".into(),
    }
}
