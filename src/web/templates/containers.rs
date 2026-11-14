//! Container inventory, run form, and dialog templates.

use maud::{html, Markup};

use hakobi::catalog::ProviderConnection;
use hakobi::providers::{ContainerInventory, ContainerRow};

use super::{brand, page, sidebar, CONTENT_WIDTH, EYEBROW, FIELD};

pub fn containers(
    base_url: &str,
    username: &str,
    csrf_token: &str,
    groups: &[(ProviderConnection, Result<ContainerInventory, String>)],
    notice: Option<&str>,
) -> Markup {
    page(
        "Containers · Hakobi",
        html! {
            div class="min-h-screen lg:pl-60" {
                (sidebar("containers", base_url, username, csrf_token))
                (containers_mobile_header(username, csrf_token))
                (containers_main(base_url, csrf_token, groups, notice))
            }
        },
    )
}

fn containers_mobile_header(username: &str, csrf_token: &str) -> Markup {
    html! {
        header class="lg:hidden border-b border-line bg-paper/95" {
            div class=(format!("{CONTENT_WIDTH} flex h-[5.5rem] items-center justify-between gap-4")) {
                (brand("Hakobi dashboard"))
                div class="flex items-center gap-4" {
                    a class="text-[0.8rem] font-semibold text-muted no-underline hover:text-ink" href="/dashboard" { "Back to overview" }
                    a class="text-[0.8rem] font-semibold text-muted no-underline hover:text-ink" href="/providers" { "Providers" }
                    span class="text-sm text-muted max-sm:hidden" { (username) }
                    form method="post" action="/logout" class="m-0" {
                        input type="hidden" name="csrf_token" value=(csrf_token);
                        button class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.8rem] font-semibold text-ink" type="submit" { "Sign out" }
                    }
                }
            }
        }
    }
}

fn containers_main(
    base_url: &str,
    csrf_token: &str,
    groups: &[(ProviderConnection, Result<ContainerInventory, String>)],
    notice: Option<&str>,
) -> Markup {
    html! {
        main class=(format!("{CONTENT_WIDTH} pb-20 pt-10 lg:pt-16")) {
            div class="flex flex-col items-start gap-4 sm:flex-row sm:items-end sm:justify-between" {
                div {
                    p class=(format!("{EYEBROW} mb-3 text-brand-bright")) { "Docker" }
                    h1 class="m-0 font-display text-[clamp(2.2rem,5vw,4rem)] font-normal tracking-[-0.04em]" { "Containers" }
                }
                a class="rounded-[0.6rem] bg-brand px-4 py-3 text-[0.85rem] font-bold text-white no-underline transition hover:bg-[#0f4130]" href="/containers/new" {
                    i class="fa-solid fa-plus mr-2" aria-hidden="true" {}
                    "Run container"
                }
            }
            (delete_container_dialog(csrf_token))
            p class="mt-4 max-w-[44rem] leading-[1.7] text-muted" {
                "Live container inventory from enabled Docker providers. Start, stop, restart, or remove containers per provider. Images are never pulled and volumes are never removed."
            }
            @if let Some(message) = notice {
                p class="mb-6 mt-6 rounded-[0.6rem] border border-line bg-surface/60 px-4 py-3 text-sm" role="status" { (message) }
            }
            (container_provider_groups(groups, csrf_token))
            p class="mt-8 text-xs text-muted" { "Dashboard endpoint: " (base_url) }
        }
    }
}

fn container_provider_groups(
    groups: &[(ProviderConnection, Result<ContainerInventory, String>)],
    csrf_token: &str,
) -> Markup {
    html! {
        @if groups.is_empty() {
            div class="mt-8 rounded-[0.85rem] border border-dashed border-line p-8 text-center text-muted" {
                img src="/assets/art/illustrations/empty-containers.webp" alt="" aria-hidden="true" class="empty-art";
                p class="m-0" {
                    "No enabled Docker providers. "
                    a class="font-semibold text-brand" href="/providers" { "Connect one first." }
                }
            }
        }
        @for (provider, result) in groups {
            (provider_section(provider, result, csrf_token))
        }
    }
}

fn provider_section(
    provider: &ProviderConnection,
    result: &Result<ContainerInventory, String>,
    csrf_token: &str,
) -> Markup {
    html! {
        section class="mt-8" {
            div class="mb-3 flex flex-col gap-1 sm:flex-row sm:items-end sm:justify-between" {
                h2 class="m-0 text-[1.2rem] font-bold" { (provider.name) }
                span class="break-all font-mono text-xs text-muted" { (provider.endpoint) }
            }
            @match result {
                Ok(inventory) => (provider_inventory(provider, inventory, csrf_token)),
                Err(error) => {
                    div class="rounded-[0.85rem] border border-danger/30 bg-danger/5 p-5 text-sm text-danger" {
                        strong class="block" { "Provider read failed" }
                        span class="mt-1 block break-words" { (error) }
                    }
                }
            }
        }
    }
}

fn provider_inventory(
    provider: &ProviderConnection,
    inventory: &ContainerInventory,
    csrf_token: &str,
) -> Markup {
    html! {
        @if !inventory.suggested_ports.is_empty() {
            div class="mb-4 flex flex-wrap items-center gap-2 text-xs text-muted" {
                span { "Next free host ports (Docker-visible):" }
                @for port in &inventory.suggested_ports {
                    code class="rounded-md border border-line bg-surface px-2 py-1 font-mono text-ink" { (port) }
                }
            }
        }
        @if inventory.groups.is_empty() {
            p class="rounded-[0.85rem] border border-dashed border-line p-6 text-sm text-muted" { "No containers found." }
        } @else {
            @for group in &inventory.groups {
                (container_group(provider, group, csrf_token))
            }
        }
    }
}

fn container_group(
    provider: &ProviderConnection,
    group: &hakobi::providers::ContainerGroup,
    csrf_token: &str,
) -> Markup {
    html! {
        section class="mt-5 first:mt-0" {
            div class="mb-2 flex items-center justify-between gap-3" {
                h3 class="m-0 text-[0.95rem] font-bold" {
                    i class="fa-solid fa-layer-group mr-2 text-muted" aria-hidden="true" {}
                    (group.name)
                }
                span class="text-[0.68rem] text-muted" { (group.containers.len()) " container" @if group.containers.len() != 1 { "s" } }
            }
            div class="overflow-x-auto rounded-[0.85rem] border border-line" {
                table class="w-full border-collapse text-left text-sm" {
                    (container_table_header())
                    tbody {
                        @for container in &group.containers {
                            (container_row(provider, container, csrf_token))
                        }
                    }
                }
            }
        }
    }
}

fn container_table_header() -> Markup {
    html! {
        thead class="bg-surface/60 text-[0.65rem] uppercase tracking-[0.12em] text-muted" {
            tr {
                th class="px-4 py-3 font-semibold" { "Container" }
                th class="px-4 py-3 font-semibold" { "Image" }
                th class="px-4 py-3 font-semibold" { "State" }
                th class="px-4 py-3 font-semibold" { "Health" }
                th class="px-4 py-3 font-semibold" { "Ports" }
                th class="px-4 py-3 font-semibold" { "Actions" }
            }
        }
    }
}

fn container_row(
    provider: &ProviderConnection,
    container: &ContainerRow,
    csrf_token: &str,
) -> Markup {
    html! {
        tr class="border-t border-line" {
            td class="px-4 py-3" {
                a class="block font-semibold no-underline hover:text-brand" href=(format!("/containers/{}/{}", provider.id, container.name)) { (container.name) }
                span class="font-mono text-[0.68rem] text-muted" { (container.id.chars().take(12).collect::<String>()) }
                @if let Some(service) = &container.compose_service {
                    span class="ml-2 text-[0.65rem] text-muted" { (service) }
                }
            }
            td class="max-w-[18rem] truncate px-4 py-3 font-mono text-xs text-muted" { (container.image) }
            td class="px-4 py-3" {
                span class="rounded-full bg-brand-bright/10 px-2.5 py-1 text-[0.68rem] font-semibold text-brand" { (container.state) }
            }
            td class="px-4 py-3 text-xs text-muted" {
                (container.health.as_deref().unwrap_or("—"))
            }
            td class="px-4 py-3 font-mono text-[0.7rem] text-muted" {
                (container_ports(container))
            }
            td class="px-4 py-3" {
                @if provider.kind == "docker" {
                    (container_actions(&provider.id, container, csrf_token))
                } @else {
                    span class="text-xs text-muted" { "—" }
                }
            }
        }
    }
}

fn container_ports(container: &ContainerRow) -> Markup {
    html! {
        @if container.ports.is_empty() {
            "—"
        } @else {
            @for port in &container.ports {
                span class="block whitespace-nowrap" {
                    @if let Some(host_port) = port.host_port {
                        (port.host_ip.as_deref().unwrap_or("0.0.0.0")) ":" (host_port) " → "
                    }
                    (port.container_port) "/" (port.protocol)
                }
            }
        }
    }
}

pub(crate) fn container_actions(
    provider: &str,
    container: &ContainerRow,
    csrf_token: &str,
) -> Markup {
    let base = format!("/containers/{provider}/{}", container.name);
    let container_name = container.name.as_str();
    html! {
        div class="flex flex-wrap items-center gap-1.5" {
            form method="post" action=(format!("{base}/start")) class="m-0" {
                input type="hidden" name="csrf_token" value=(csrf_token);
                button class="rounded-md border border-line bg-transparent px-2.5 py-1.5 text-[0.68rem] font-semibold text-ink" type="submit" title=(format!("Start {container_name}")) {
                    i class="fa-solid fa-play mr-1" aria-hidden="true" {}
                    "Start"
                }
            }
            form method="post" action=(format!("{base}/stop")) class="m-0" {
                input type="hidden" name="csrf_token" value=(csrf_token);
                button class="rounded-md border border-line bg-transparent px-2.5 py-1.5 text-[0.68rem] font-semibold text-ink" type="submit" title=(format!("Stop {container_name}")) {
                    i class="fa-solid fa-stop mr-1" aria-hidden="true" {}
                    "Stop"
                }
            }
            form method="post" action=(format!("{base}/restart")) class="m-0" {
                input type="hidden" name="csrf_token" value=(csrf_token);
                button class="rounded-md border border-line bg-transparent px-2.5 py-1.5 text-[0.68rem] font-semibold text-ink" type="submit" title=(format!("Restart {container_name}")) {
                    i class="fa-solid fa-rotate-right mr-1" aria-hidden="true" {}
                    "Restart"
                }
            }
            button type="button" class="rounded-md border border-danger/40 bg-transparent px-2.5 py-1.5 text-[0.68rem] font-semibold text-danger" data-dialog-open="delete-container" data-delete-action=(format!("{base}/delete")) data-delete-name=(container_name) {
                i class="fa-solid fa-trash-can mr-1" aria-hidden="true" {}
                "Delete"
            }
        }
    }
}

pub(crate) fn delete_container_dialog(csrf_token: &str) -> Markup {
    html! {
        dialog id="delete-container" class="w-[90%] max-w-[44rem] rounded-[0.85rem] border border-line bg-surface p-6 text-ink" aria-labelledby="delete-container-title" {
            form method="post" action="/containers" class="m-0" {
                input type="hidden" name="csrf_token" value=(csrf_token);
                h2 id="delete-container-title" class="m-0 font-display text-[1.5rem] font-normal" { "Remove container?" }
                p class="mt-3 text-sm leading-relaxed text-muted" {
                    "This stops and removes " strong class="text-ink" data-confirm-name { "this container" } ". Attached volumes are preserved."
                }
                label class="mt-4 flex flex-col gap-2 text-[0.8rem] font-bold" {
                    "Type the container name to confirm"
                    input class=(FIELD) name="confirm_name" type="text" autocomplete="off" data-confirm-input aria-describedby="delete-container-help" required;
                }
                p id="delete-container-help" class="mb-0 mt-2 text-xs text-muted" { "The name must match exactly. Images and volumes are kept." }
                div class="mt-5 flex flex-wrap gap-2" {
                    button type="button" class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-ink" data-dialog-close { "Cancel" }
                    button type="submit" class="rounded-lg border border-danger/40 bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-danger" data-confirm-submit disabled { "Remove container" }
                }
            }
        }
    }
}

pub struct ContainerFormValues {
    pub provider_id: String,
    pub image: String,
    pub name: String,
    pub container_port: String,
}

pub fn container_form(
    base_url: &str,
    username: &str,
    csrf_token: &str,
    providers: &[ProviderConnection],
    values: &ContainerFormValues,
    error: Option<&str>,
) -> Markup {
    page(
        "Run container · Hakobi",
        html! {
            div class="min-h-screen lg:pl-60" {
                (sidebar("containers", base_url, username, csrf_token))
                (container_form_mobile_header(username, csrf_token))
                (container_form_main(providers, values, error, csrf_token))
            }
        },
    )
}

fn container_form_mobile_header(username: &str, csrf_token: &str) -> Markup {
    html! {
        header class="lg:hidden border-b border-line bg-paper/95" {
            div class=(format!("{CONTENT_WIDTH} flex h-[5.5rem] items-center justify-between gap-4")) {
                (brand("Hakobi dashboard"))
                div class="flex items-center gap-4" {
                    a class="text-[0.8rem] font-semibold text-muted no-underline hover:text-ink" href="/containers" { "Containers" }
                    span class="text-sm text-muted max-sm:hidden" { (username) }
                    form method="post" action="/logout" class="m-0" {
                        input type="hidden" name="csrf_token" value=(csrf_token);
                        button class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.8rem] font-semibold text-ink" type="submit" { "Sign out" }
                    }
                }
            }
        }
    }
}

fn container_form_main(
    providers: &[ProviderConnection],
    values: &ContainerFormValues,
    error: Option<&str>,
    csrf_token: &str,
) -> Markup {
    html! {
        main class=(format!("{CONTENT_WIDTH} pb-20 pt-10 lg:pt-16")) {
            a class="text-[0.75rem] font-semibold text-muted no-underline hover:text-ink" href="/containers" { "← All containers" }
            p class=(format!("{EYEBROW} mb-3 mt-6 text-brand-bright")) { "Docker" }
            h1 class="m-0 font-display text-[clamp(2.2rem,5vw,4rem)] font-normal tracking-[-0.04em]" { "Run container" }
            p class="mt-4 max-w-[44rem] leading-[1.7] text-muted" {
                "Starts a container from an image that already exists on the provider. Hakobi never pulls images, never deletes volumes, and assigns the next free host port in 18000–18200."
            }
            @if providers.is_empty() {
                p class="mt-8 rounded-[0.85rem] border border-dashed border-line p-8 text-center text-muted" {
                    "No enabled Docker providers. "
                    a class="font-semibold text-brand" href="/providers" { "Connect one first." }
                }
            } @else {
                @if let Some(message) = error {
                    div class="mb-6 mt-6 border-l-[3px] border-danger bg-danger/5 px-4 py-3.5 text-sm leading-normal text-danger" role="alert" { (message) }
                }
                (container_form_fields(providers, values, csrf_token))
            }
        }
    }
}

fn container_form_fields(
    providers: &[ProviderConnection],
    values: &ContainerFormValues,
    csrf_token: &str,
) -> Markup {
    html! {
        form method="post" action="/containers/new" class="mt-8 grid max-w-[44rem] grid-cols-1 gap-5 rounded-[0.85rem] border border-line bg-surface/45 p-6" {
            input type="hidden" name="csrf_token" value=(csrf_token);
            (container_provider_fields(providers, values))
            (container_run_fields(values))
        }
    }
}

fn container_provider_fields(
    providers: &[ProviderConnection],
    values: &ContainerFormValues,
) -> Markup {
    html! {
        label class="flex flex-col gap-2 text-[0.8rem] font-bold" {
            "Provider"
            select class=(FIELD) name="provider_id" required {
                option value="" disabled selected[values.provider_id.is_empty()] { "Select an enabled provider…" }
                @for provider in providers {
                    option value=(provider.id) selected[values.provider_id == provider.id] {
                        (provider.name) " · " (provider.endpoint)
                    }
                }
            }
        }
        label class="flex flex-col gap-2 text-[0.8rem] font-bold" {
            "Image"
            p class="m-0 text-xs font-normal leading-relaxed text-muted" {
                "An image reference already present locally, e.g. nginx:latest. Missing images fail instead of pulling."
            }
            input class=(FIELD) name="image" type="text" value=(values.image) maxlength="255" placeholder="nginx:latest" required autocomplete="off";
        }
    }
}

fn container_run_fields(values: &ContainerFormValues) -> Markup {
    html! {
        div class="grid grid-cols-1 gap-5 md:grid-cols-2" {
            label class="flex flex-col gap-2 text-[0.8rem] font-bold" {
                "Name"
                p class="m-0 text-xs font-normal leading-relaxed text-muted" {
                    "1–128 letters, digits, dots, dashes, or underscores. Must be unique on the provider."
                }
                input class=(FIELD) name="name" type="text" value=(values.name) maxlength="128" pattern="[A-Za-z0-9][A-Za-z0-9._-]*" required autocomplete="off";
            }
            label class="flex flex-col gap-2 text-[0.8rem] font-bold" {
                "Container port"
                p class="m-0 text-xs font-normal leading-relaxed text-muted" {
                    "Port the container listens on. Hakobi publishes it on a free host port."
                }
                input class=(FIELD) name="container_port" type="number" value=(values.container_port) min="1" max="65535" required autocomplete="off";
            }
        }
        div class="flex flex-wrap items-center gap-3" {
            button class="rounded-[0.6rem] bg-brand px-4 py-3 font-bold text-white transition hover:bg-[#0f4130]" type="submit" { "Run container" }
            a class="text-[0.8rem] font-semibold text-muted no-underline hover:text-ink" href="/containers" { "Cancel" }
        }
    }
}
