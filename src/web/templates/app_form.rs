//! App create/edit form template and delete confirmation dialog.

mod fields;
mod wizard;

use maud::{html, Markup};

use hakobi::catalog::HostAgent;

use super::{brand, page, sidebar, CONTENT_WIDTH, EYEBROW, FIELD};
use fields::{app_identity_fields, build_settings_fields, github_source_fields};
use wizard::{deployment_policy_field, resource_limit_fields, review_summary};

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
    pub requested_host_port: String,
    pub cpu_limit: String,
    pub memory_limit_mb: String,
    pub env_json: String,
}

pub(super) const CONTROL: &str = "w-full rounded-lg border border-zinc-200 bg-white px-3 py-2.5 text-[13px] text-zinc-950 outline-none transition placeholder:text-zinc-400 focus:border-[#165237] focus:ring-[3px] focus:ring-[#165237]/10";
const PRIMARY_BUTTON: &str = "inline-flex min-h-10 items-center justify-center gap-2 rounded-lg bg-[#165237] px-3.5 py-2 text-[13px] font-semibold text-white transition hover:bg-[#11402b]";
const SECONDARY_BUTTON: &str = "inline-flex min-h-10 items-center justify-center rounded-lg border border-zinc-200 bg-white px-3.5 py-2 text-[13px] font-semibold text-zinc-700 transition hover:bg-zinc-50";

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
        &format!("{heading} · Hakobi"),
        html! {
            div class="min-h-screen bg-[#fafafa] lg:pl-60" {
                (sidebar("apps", base_url, username, csrf_token))
                (app_form_mobile_header(username, csrf_token))
                (breadcrumb_bar(heading, action == "/apps"))
                (app_form_main(hosts, values, action, heading, submit_label, error, csrf_token))
            }
        },
    )
}

fn app_form_mobile_header(username: &str, csrf_token: &str) -> Markup {
    html! {
        header class="lg:hidden border-b border-line bg-paper/95" {
            div class=(format!("{CONTENT_WIDTH} flex h-[5.5rem] items-center justify-between gap-4")) {
                (brand("Hakobi dashboard"))
                div class="flex items-center gap-4" {
                    a class="text-[0.8rem] font-semibold text-muted no-underline hover:text-ink" href="/apps" { "Apps" }
                    span class="max-sm:hidden text-sm text-muted" { (username) }
                    form method="post" action="/logout" class="m-0" {
                        input type="hidden" name="csrf_token" value=(csrf_token);
                        button class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.8rem] font-semibold text-ink" type="submit" { "Sign out" }
                    }
                }
            }
        }
    }
}

fn breadcrumb_bar(heading: &str, guided: bool) -> Markup {
    html! {
        div class="border-b border-zinc-200 bg-white/80" {
            div class="mx-auto flex h-14 w-full max-w-5xl items-center justify-between gap-4 px-4 sm:px-8" {
                nav class="flex min-w-0 items-center gap-2 text-[12px]" aria-label="Breadcrumb" {
                    a class="font-medium text-zinc-500 no-underline hover:text-zinc-950" href="/apps" { "All apps" }
                    span class="text-zinc-300" aria-hidden="true" { "/" }
                    span class="truncate font-medium text-zinc-900" aria-current="page" { (heading) }
                }
                @if guided {
                    span class="inline-flex shrink-0 items-center gap-1.5 rounded-full border border-[#165237]/15 bg-[#165237]/5 px-2.5 py-1 whitespace-nowrap text-[10px] font-semibold text-[#165237]" {
                        i class="fa-solid fa-wand-magic-sparkles" aria-hidden="true" {}
                        "Build Recipe Wizard"
                    }
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn app_form_main(
    hosts: &[HostAgent],
    values: &AppFormValues,
    action: &str,
    heading: &str,
    submit_label: &str,
    error: Option<&str>,
    csrf_token: &str,
) -> Markup {
    let guided = action == "/apps";
    html! {
        main class="mx-auto w-full max-w-5xl px-4 py-7 sm:px-8 sm:py-9" {
            p class=(format!("{EYEBROW} m-0 text-[#165237]")) { "App" }
            h1 class="m-0 mt-2 font-display text-[34px] font-normal leading-[1.1] tracking-[-0.03em] text-zinc-950" { (heading) }
            p class="mb-0 mt-3 max-w-2xl text-[13.5px] leading-relaxed text-zinc-500" {
                @if guided {
                    "Create a build recipe from a GitHub repository and choose where Hakobi should run it."
                } @else {
                    "Update this app’s source, runtime, resources, and deployment policy."
                }
            }
            @if let Some(message) = error {
                div class="mt-5 rounded-lg border border-danger/20 bg-danger/5 px-4 py-3 text-[13px] leading-normal text-danger" role="alert" { (message) }
            }
            (app_form_fields(hosts, values, action, submit_label, csrf_token, guided))
        }
    }
}

fn app_form_fields(
    hosts: &[HostAgent],
    values: &AppFormValues,
    action: &str,
    submit_label: &str,
    csrf_token: &str,
    guided: bool,
) -> Markup {
    html! {
        form method="post" action=(action) data-app-form data-app-stepper[guided] class="mt-6" {
            input type="hidden" name="csrf_token" value=(csrf_token);
            @if guided {
                (wizard::stepper_navigation())
                p data-step-error role="alert" class="mb-0 mt-3 min-h-5 text-[13px] text-danger" {}
            }
            div class=(if guided { "mt-2 grid grid-cols-1 gap-6 lg:grid-cols-12 lg:items-start" } else { "mt-6" }) {
                div class="divide-y divide-zinc-200 overflow-hidden rounded-xl border border-zinc-200 bg-white shadow-sm lg:col-span-8" {
                    (step_panel(1, "Source", "Choose a repository and application. Hakobi fills in what it can detect.", github_source_fields(values), guided, submit_label))
                    (step_panel(2, "Name your app", "Choose a recognizable name and an enrolled host for the app.", app_identity_fields(hosts, values), guided, submit_label))
                    (step_panel(3, "Configure the build", "Set the runtime, port, environment, and build paths.", build_settings_fields(values), guided, submit_label))
                    (step_panel(4, "Resources and review", "Choose an allocation and deployment policy, then review the recipe.", resources_and_review(values, guided), guided, submit_label))
                }
                @if guided { (wizard::recipe_summary(hosts, values)) }
            }
        }
    }
}

fn resources_and_review(values: &AppFormValues, guided: bool) -> Markup {
    html! {
        (resource_limit_fields(values))
        (deployment_policy_field(values))
        @if guided { (review_summary(values)) }
    }
}

fn step_panel(
    number: u8,
    title: &str,
    description: &str,
    body: Markup,
    guided: bool,
    submit_label: &str,
) -> Markup {
    html! {
        section data-app-step=(number) class="bg-white" {
            div class="flex flex-col gap-3 border-b border-zinc-200 px-6 py-5 sm:flex-row sm:items-start sm:justify-between" {
                div {
                    h2 tabindex="-1" class="m-0 font-display text-[22px] font-normal leading-tight tracking-[-0.02em] text-zinc-950 outline-none" { (title) }
                    p class="mb-0 mt-2 text-[12.5px] leading-relaxed text-zinc-500" { (description) }
                }
                @if guided {
                    span class="shrink-0 pt-1 text-[11px] font-medium text-zinc-400" { "Step " (number) " of 4" }
                }
            }
            div class="grid gap-5 p-6" { (body) }
            @if guided {
                (step_navigation(number, submit_label))
            } @else if number == 4 {
                (edit_actions(submit_label))
            }
        }
    }
}

fn step_navigation(step: u8, submit_label: &str) -> Markup {
    html! {
        div data-step-navigation class="flex items-center justify-between gap-2 border-t border-zinc-200 bg-zinc-50 px-4 py-4 sm:px-6" {
            button type="button" data-step-back hidden[step == 1] class=(SECONDARY_BUTTON) {
                i class="fa-solid fa-arrow-left mr-2 text-[10px]" aria-hidden="true" {}
                "Back"
            }
            @if step < 4 {
                button type="button" data-step-next class=(PRIMARY_BUTTON) {
                    "Continue"
                    i class="fa-solid fa-arrow-right text-[10px]" aria-hidden="true" {}
                }
            } @else {
                div data-step-submit class="flex items-center gap-2" {
                    a class="px-1 text-[12px] font-semibold text-zinc-500 no-underline hover:text-zinc-950" href="/apps" { "Cancel" }
                    button class=(PRIMARY_BUTTON) type="submit" { (submit_label) }
                }
            }
        }
    }
}

fn edit_actions(submit_label: &str) -> Markup {
    html! {
        div class="flex items-center justify-end gap-3 border-t border-zinc-200 bg-zinc-50 px-6 py-4" data-step-submit {
            a class="text-[12px] font-semibold text-zinc-500 no-underline hover:text-zinc-950" href="/apps" { "Cancel" }
            button class=(PRIMARY_BUTTON) type="submit" { (submit_label) }
        }
    }
}

pub(crate) fn delete_app_dialog(app_id: &str, csrf_token: &str) -> Markup {
    html! {
        dialog id="delete-app" class="w-[90%] max-w-[44rem] rounded-[0.85rem] border border-line bg-surface p-6 text-ink" aria-labelledby="delete-app-title" {
            form method="post" action=(format!("/apps/{app_id}/delete")) class="m-0" {
                input type="hidden" name="csrf_token" value=(csrf_token);
                h2 id="delete-app-title" class="m-0 font-display text-[1.5rem] font-normal" { "Delete app?" }
                p class="mt-3 text-sm leading-relaxed text-muted" {
                    "This permanently removes " strong class="text-ink" data-confirm-name { "this app" } ", its container, deployment images, deployments, and logs. App-owned networks are always deleted. App-owned volumes and their data are preserved unless you explicitly choose data deletion below."
                }
                label class="mt-4 flex items-start gap-2 text-sm font-semibold text-danger" {
                    input type="checkbox" name="delete_data" value="on" class="mt-1";
                    span { "Permanently delete app-owned volumes and all data" }
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
