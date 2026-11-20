use hakobi::catalog::HostAgent;
use maud::{html, Markup};

use super::fields::{
    app_identity_fields, build_configuration_fields, environment_fields, github_source_fields,
};
use super::runtime::{data_fields, process_fields};
use super::wizard::{deployment_policy_field, resource_limit_fields};
use super::{AppFormValues, PRIMARY_BUTTON, SECONDARY_BUTTON};

const TABS: [(&str, &str, &str); 5] = [
    (
        "general",
        "General",
        "Name, host, allocation, and deployment policy.",
    ),
    (
        "source",
        "Source",
        "Repository, branch or tag, and connected-source detection.",
    ),
    (
        "build",
        "Build",
        "Image build, loopback port mapping, and repository paths.",
    ),
    (
        "processes",
        "Processes",
        "Web and release commands plus private workers.",
    ),
    (
        "data",
        "Environment & data",
        "Variables, encrypted secrets, and persistent volumes.",
    ),
];

pub(super) fn edit_settings(
    hosts: &[HostAgent],
    values: &AppFormValues,
    action: &str,
    submit_label: &str,
) -> Markup {
    html! {
        input type="hidden" name="settings_tab" value=(&values.settings_tab) data-settings-tab-input;
        nav class="mt-6 flex gap-5 overflow-x-auto border-b border-zinc-200" aria-label="Application settings" data-settings-navigation {
            @for (tab, label, _) in TABS {
                a href=(format!("{action}?tab={tab}")) data-settings-link=(tab) data-state=(if values.settings_tab == tab { "active" } else { "inactive" }) aria-current=(if values.settings_tab == tab { "page" } else { "false" }) class="shrink-0 border-b-2 border-transparent px-0.5 py-3 text-[12.5px] font-semibold text-zinc-500 no-underline hover:text-zinc-950 data-[state=active]:border-[#165237] data-[state=active]:text-[#165237]" { (label) }
            }
        }
        div class="mt-5 overflow-hidden rounded-xl border border-zinc-200 bg-white shadow-sm" {
            @for (tab, title, description) in TABS {
                (settings_panel(tab, title, description, values, tab_body(tab, hosts, values)))
            }
        }
        div class="sticky bottom-3 z-10 mt-4 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-zinc-200 bg-white/95 px-4 py-3 shadow-sm backdrop-blur" data-settings-footer {
            div class="min-w-0" {
                p data-settings-clean class="m-0 text-[12px] text-zinc-500" { "Changes apply to future deployments." }
                p data-settings-dirty hidden class="m-0 text-[12px] font-semibold text-[#165237]" { "Unsaved changes" }
            }
            div class="flex items-center gap-2" {
                a href=(format!("{action}?tab={}", values.settings_tab)) class=(SECONDARY_BUTTON) data-settings-discard { "Discard" }
                button type="submit" class=(PRIMARY_BUTTON) { (submit_label) }
            }
        }
    }
}

fn tab_body(tab: &str, hosts: &[HostAgent], values: &AppFormValues) -> Markup {
    match tab {
        "general" => html! {
            (app_identity_fields(hosts, values))
            (resource_limit_fields(values))
            (deployment_policy_field(values))
        },
        "source" => github_source_fields(values),
        "build" => build_configuration_fields(values),
        "processes" => process_fields(values),
        "data" => html! { (environment_fields(values)) (data_fields(values)) },
        _ => html! {},
    }
}

fn settings_panel(
    tab: &str,
    title: &str,
    description: &str,
    values: &AppFormValues,
    body: Markup,
) -> Markup {
    html! {
        section data-settings-panel=(tab) hidden[values.settings_tab != tab] aria-labelledby=(format!("settings-{tab}")) {
            div class="border-b border-zinc-200 px-6 py-5" {
                h2 id=(format!("settings-{tab}")) tabindex="-1" class="m-0 font-display text-[22px] font-normal text-zinc-950 outline-none" { (title) }
                p class="mb-0 mt-2 text-[12.5px] leading-relaxed text-zinc-500" { (description) }
            }
            div class="grid gap-5 p-6" { (body) }
        }
    }
}
