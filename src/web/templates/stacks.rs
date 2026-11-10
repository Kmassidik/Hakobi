//! Managed and runtime stack templates.

use maud::{html, Markup};

use hakobi::catalog::ProviderConnection;
use hakobi::providers::{ContainerInventory, StackGroup};
use hakobi::stacks::{StackDefinition, StackKind};

use super::containers::{container_actions, container_ports, delete_container_dialog};
use super::{brand, page, sidebar, CONTENT_WIDTH, EYEBROW, FIELD};

pub fn stacks(
    base_url: &str,
    username: &str,
    csrf_token: &str,
    groups: &[(ProviderConnection, Result<ContainerInventory, String>)],
    providers: &[ProviderConnection],
    managed: &[StackDefinition],
    notice: Option<&str>,
) -> Markup {
    page(
        "Stacks · Hakobi",
        html! {
            div class="min-h-screen lg:pl-60" {
                (sidebar("stacks", base_url, username, csrf_token))
                header class="lg:hidden border-b border-line bg-paper/95" {
                    div class=(format!("{CONTENT_WIDTH} flex h-[5.5rem] items-center justify-between gap-4")) {
                        (brand("Hakobi dashboard"))
                        div class="flex items-center gap-4" {
                            a class="text-[0.8rem] font-semibold text-muted no-underline hover:text-ink" href="/" { "Overview" }
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
                            p class=(format!("{EYEBROW} mb-3 text-brand-bright")) { "Docker" }
                            h1 class="m-0 font-display text-[clamp(2.2rem,5vw,4rem)] font-normal tracking-[-0.04em]" { "Stacks" }
                        }
                        a class="rounded-[0.6rem] bg-brand px-4 py-3 text-[0.85rem] font-bold text-white no-underline transition hover:bg-[#0f4130]" href="/stacks/new" {
                            i class="fa-solid fa-plus mr-2" aria-hidden="true" {}
                            "New stack"
                        }
                    }
                    p class="mt-4 max-w-[44rem] leading-[1.7] text-muted" {
                        "Managed definitions describe what to deploy. Runtime stacks below show what Docker is actually running."
                    }
                    @if let Some(message) = notice {
                        p class="mb-6 mt-6 rounded-[0.6rem] border border-line bg-surface/60 px-4 py-3 text-sm" role="status" { (message) }
                    }
                    section class="mt-8" aria-labelledby="managed-stacks-title" {
                        div class="mb-3 flex flex-col gap-1 sm:flex-row sm:items-end sm:justify-between" {
                            h2 id="managed-stacks-title" class="m-0 text-[1.2rem] font-bold" { "Managed stacks" }
                            p class="m-0 text-xs text-muted" { "Definitions only. Nothing is deployed from this page yet." }
                        }
                        @if managed.is_empty() {
                            p class="rounded-[0.85rem] border border-dashed border-line p-6 text-sm text-muted" {
                                "No managed stack definitions yet. "
                                a class="font-semibold text-brand" href="/stacks/new" { "Create one." }
                            }
                        } @else {
                            div class="grid gap-4" {
                                @for stack in managed {
                                    article class="rounded-[0.85rem] border border-line bg-surface/45 p-5" {
                                        div class="flex flex-col justify-between gap-3 sm:flex-row sm:items-start" {
                                            div class="min-w-0" {
                                                h3 class="m-0 text-[1.15rem] font-bold" {
                                                    i class="fa-solid fa-file-code mr-2 text-muted" aria-hidden="true" {}
                                                    (stack.name)
                                                }
                                                p class="mb-0 mt-1 break-words text-xs text-muted" {
                                                    (stack.kind.as_str()) " · "
                                                    @if let Some(provider) = providers.iter().find(|provider| provider.id == stack.provider_id) {
                                                        (provider.name)
                                                    } @else {
                                                        (stack.provider_id)
                                                    }
                                                    " · source: " (stack.source)
                                                }
                                            }
                                            span class="inline-flex w-fit items-center gap-2 rounded-full bg-brand-bright/10 px-3 py-1 text-xs font-semibold text-brand" {
                                                (stack.kind.as_str())
                                            }
                                        }
                                        p class="mb-0 mt-3 text-[0.7rem] text-muted" {
                                            "Updated " (stack.updated_at)
                                        }
                                        div class="mt-4 flex flex-wrap gap-2" {
                                            a class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-ink no-underline" href=(format!("/stacks/{}/edit", stack.id)) {
                                                i class="fa-solid fa-pen mr-2" aria-hidden="true" {}
                                                "Edit"
                                            }
                                            button type="button" class="rounded-lg border border-danger/40 bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-danger" data-dialog-open="delete-stack" data-delete-action=(format!("/stacks/{}/delete", stack.id)) data-delete-name=(stack.name) {
                                                i class="fa-solid fa-trash-can mr-2" aria-hidden="true" {}
                                                "Delete"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    (delete_stack_dialog(csrf_token))
                    @if groups.is_empty() {
                        div class="mt-8 rounded-[0.85rem] border border-dashed border-line p-8 text-center text-muted" {
                            img src="/assets/art/illustrations/empty-stacks.webp" alt="" aria-hidden="true" class="empty-art";
                            p class="m-0" {
                                "No enabled Docker providers. "
                                a class="font-semibold text-brand" href="/providers" { "Connect one first." }
                            }
                        }
                    }
                    @for (provider, result) in groups {
                        section class="mt-10" {
                            div class="mb-4 flex flex-col gap-1 sm:flex-row sm:items-end sm:justify-between" {
                                h2 class="m-0 text-[1.2rem] font-bold" { (provider.name) }
                                span class="break-all font-mono text-xs text-muted" { (provider.endpoint) }
                            }
                            @match result {
                                Ok(inventory) => {
                                    @if !inventory.suggested_ports.is_empty() {
                                        div class="mb-4 flex flex-wrap items-center gap-2 text-xs text-muted" {
                                            span { "Next free host ports (Docker-visible):" }
                                            @for port in &inventory.suggested_ports {
                                                code class="rounded-md border border-line bg-surface px-2 py-1 font-mono text-ink" { (port) }
                                            }
                                        }
                                    }
                                    @if inventory.stacks.is_empty() {
                                        p class="rounded-[0.85rem] border border-dashed border-line p-6 text-sm text-muted" { "No stacks found." }
                                    } @else {
                                        div class="grid gap-4" {
                                            @for stack in &inventory.stacks {
                                                article class="rounded-[0.85rem] border border-line bg-surface/45 p-5" {
                                                    div class="flex flex-col justify-between gap-3 sm:flex-row sm:items-start" {
                                                        div {
                                                            h3 class="m-0 text-[1.15rem] font-bold" {
                                                                i class="fa-solid fa-layer-group mr-2 text-muted" aria-hidden="true" {}
                                                                (stack.name)
                                                            }
                                                            p class="mb-0 mt-1 text-xs text-muted" {
                                                                (stack.services.len()) " service" @if stack.services.len() != 1 { "s" }
                                                                " · " (stack.running_count) "/" (stack.container_count) " containers running"
                                                            }
                                                        }
                                                        a class="text-[0.78rem] font-semibold text-brand no-underline hover:underline" href=(format!("/stacks/runtime/{}/{}", provider.id, stack.name)) { "Open stack →" }
                                                    }
                                                    div class="mt-4 overflow-x-auto" {
                                                        table class="w-full border-collapse text-left text-xs" {
                                                            thead class="text-[0.6rem] uppercase tracking-[0.1em] text-muted" {
                                                                tr {
                                                                    th class="pb-2 pr-3 font-semibold" { "Service" }
                                                                    th class="pb-2 pr-3 font-semibold" { "Image" }
                                                                    th class="pb-2 pr-3 font-semibold" { "Replicas" }
                                                                    th class="pb-2 pr-3 font-semibold" { "Ports" }
                                                                    th class="pb-2 font-semibold" { "Health" }
                                                                }
                                                            }
                                                            tbody {
                                                                @for service in &stack.services {
                                                                    @if let Some(container) = service.containers.first() {
                                                                        tr class="border-t border-line" {
                                                                            td class="py-2 pr-3 font-semibold" { (service.name) }
                                                                            td class="max-w-[18rem] truncate py-2 pr-3 font-mono text-muted" { (container.image) }
                                                                            td class="py-2 pr-3 text-muted" { (service.containers.len()) }
                                                                            td class="py-2 pr-3 font-mono text-muted" { (container_ports(container)) }
                                                                            td class="py-2 text-muted" { (container.health.as_deref().unwrap_or("—")) }
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
                                },
                                Err(error) => {
                                    div class="rounded-[0.85rem] border border-danger/30 bg-danger/5 p-5 text-sm text-danger" {
                                        strong class="block" { "Provider read failed" }
                                        span class="mt-1 block break-words" { (error) }
                                    }
                                }
                            }
                        }
                    }
                    p class="mt-8 text-xs text-muted" { "Dashboard endpoint: " (base_url) }
                }
            }
        },
    )
}

/// Prefilled values for the managed stack create and edit forms.
pub struct StackFormValues {
    pub kind: String,
    pub provider_id: String,
    pub name: String,
    pub source: String,
    pub yaml: String,
}

#[allow(clippy::too_many_arguments)]
pub fn stack_form(
    base_url: &str,
    username: &str,
    csrf_token: &str,
    providers: &[ProviderConnection],
    values: &StackFormValues,
    action: &str,
    heading: &str,
    submit_label: &str,
    provider_name: Option<&str>,
    error: Option<&str>,
) -> Markup {
    let title = format!("{heading} · Hakobi");
    page(
        &title,
        html! {
            div class="min-h-screen lg:pl-60" {
                (sidebar("stacks", base_url, username, csrf_token))
                header class="lg:hidden border-b border-line bg-paper/95" {
                    div class=(format!("{CONTENT_WIDTH} flex h-[5.5rem] items-center justify-between gap-4")) {
                        (brand("Hakobi dashboard"))
                        div class="flex items-center gap-4" {
                            a class="text-[0.8rem] font-semibold text-muted no-underline hover:text-ink" href="/stacks" { "Stacks" }
                            span class="text-sm text-muted max-sm:hidden" { (username) }
                            form method="post" action="/logout" class="m-0" {
                                input type="hidden" name="csrf_token" value=(csrf_token);
                                button class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.8rem] font-semibold text-ink" type="submit" { "Sign out" }
                            }
                        }
                    }
                }
                main class=(format!("{CONTENT_WIDTH} pb-20 pt-10 lg:pt-16")) {
                    a class="text-[0.75rem] font-semibold text-muted no-underline hover:text-ink" href="/stacks" { "← All stacks" }
                    p class=(format!("{EYEBROW} mb-3 mt-6 text-brand-bright")) { "Managed definition" }
                    h1 class="m-0 font-display text-[clamp(2.2rem,5vw,4rem)] font-normal tracking-[-0.04em]" { (heading) }
                    p class="mt-4 max-w-[44rem] leading-[1.7] text-muted" {
                        "Definitions are stored in the catalog. Compose and Kubernetes bodies are syntax-checked before saving; apply and deploy are not part of this step."
                    }
                    @if let Some(message) = error {
                        div class="mb-6 mt-6 border-l-[3px] border-danger bg-danger/5 px-4 py-3.5 text-sm leading-normal text-danger" role="alert" { (message) }
                    }
                    form method="post" action=(action) class="mt-8 grid max-w-[44rem] grid-cols-1 gap-5 rounded-[0.85rem] border border-line bg-surface/45 p-6" {
                        input type="hidden" name="csrf_token" value=(csrf_token);
                        div class="grid grid-cols-1 gap-5 md:grid-cols-2" {
                            label class="flex flex-col gap-2 text-[0.8rem] font-bold" {
                                "Kind"
                                select class=(FIELD) name="kind" required {
                                    @for kind in [StackKind::Dockerfile, StackKind::Compose, StackKind::Kubernetes, StackKind::Nixpacks] {
                                        option value=(kind.as_str()) selected[values.kind == kind.as_str()] { (kind.as_str()) }
                                    }
                                }
                            }
                            @if let Some(name) = provider_name {
                                div class="flex flex-col gap-2 text-[0.8rem] font-bold" {
                                    "Provider"
                                    p class="m-0 rounded-[0.6rem] border border-line bg-surface px-4 py-3.5 text-sm font-normal text-muted" {
                                        (name)
                                    }
                                    input type="hidden" name="provider_id" value=(values.provider_id);
                                }
                            } @else {
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
                            }
                        }
                        label class="flex flex-col gap-2 text-[0.8rem] font-bold" {
                            "Name"
                            p class="m-0 text-xs font-normal leading-relaxed text-muted" {
                                "1–64 ASCII letters, digits, dots, dashes, or underscores. Must be unique per provider."
                            }
                            input class=(FIELD) name="name" type="text" value=(values.name) maxlength="64" pattern="[A-Za-z0-9._-]+" required autocomplete="off";
                        }
                        label class="flex flex-col gap-2 text-[0.8rem] font-bold" {
                            "Source"
                            p class="m-0 text-xs font-normal leading-relaxed text-muted" {
                                "Dockerfile path, image reference, or repository URL. Stored verbatim."
                            }
                            input class=(FIELD) name="source" type="text" value=(values.source) maxlength="1024" required autocomplete="off";
                        }
                        label class="flex flex-col gap-2 text-[0.8rem] font-bold" {
                            "Definition YAML"
                            p class="m-0 text-xs font-normal leading-relaxed text-muted" {
                                "Compose and Kubernetes definitions must be valid YAML and are rejected before saving when malformed. Dockerfile and Nixpacks bodies are stored as written."
                            }
                            textarea class=(FIELD) name="yaml" rows="16" spellcheck="false" { (values.yaml) }
                        }
                        div class="flex flex-wrap items-center gap-3" {
                            button class="rounded-[0.6rem] bg-brand px-4 py-3 font-bold text-white transition hover:bg-[#0f4130]" type="submit" {
                                (submit_label)
                            }
                            a class="text-[0.8rem] font-semibold text-muted no-underline hover:text-ink" href="/stacks" { "Cancel" }
                        }
                    }
                }
            }
        },
    )
}

fn delete_stack_dialog(csrf_token: &str) -> Markup {
    html! {
        dialog id="delete-stack" class="w-[90%] max-w-[44rem] rounded-[0.85rem] border border-line bg-surface p-6 text-ink" aria-labelledby="delete-stack-title" {
            form method="post" action="/stacks" class="m-0" {
                input type="hidden" name="csrf_token" value=(csrf_token);
                h2 id="delete-stack-title" class="m-0 font-display text-[1.5rem] font-normal" { "Delete stack definition?" }
                p class="mt-3 text-sm leading-relaxed text-muted" {
                    "This permanently removes " strong class="text-ink" data-confirm-name { "this stack" } " from the catalog."
                }
                label class="mt-4 flex flex-col gap-2 text-[0.8rem] font-bold" {
                    "Type the stack name to confirm"
                    input class=(FIELD) name="confirm_name" type="text" autocomplete="off" data-confirm-input aria-describedby="delete-stack-help" required;
                }
                p id="delete-stack-help" class="mb-0 mt-2 text-xs text-muted" { "The name must match exactly." }
                div class="mt-5 flex flex-wrap gap-2" {
                    button type="button" class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-ink" data-dialog-close { "Cancel" }
                    button type="submit" class="rounded-lg border border-danger/40 bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-danger" data-confirm-submit disabled { "Delete stack" }
                }
            }
        }
    }
}

pub fn stack_delete(
    base_url: &str,
    username: &str,
    csrf_token: &str,
    stack: &StackDefinition,
    provider_name: &str,
    error: Option<&str>,
) -> Markup {
    page(
        "Delete stack · Hakobi",
        html! {
            div class="min-h-screen lg:pl-60" {
                (sidebar("stacks", base_url, username, csrf_token))
                header class="lg:hidden border-b border-line bg-paper/95" {
                    div class=(format!("{CONTENT_WIDTH} flex h-[5.5rem] items-center justify-between gap-4")) {
                        (brand("Hakobi dashboard"))
                        div class="flex items-center gap-4" {
                            a class="text-[0.8rem] font-semibold text-muted no-underline hover:text-ink" href="/stacks" { "Stacks" }
                            span class="text-sm text-muted max-sm:hidden" { (username) }
                            form method="post" action="/logout" class="m-0" {
                                input type="hidden" name="csrf_token" value=(csrf_token);
                                button class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.8rem] font-semibold text-ink" type="submit" { "Sign out" }
                            }
                        }
                    }
                }
                main class=(format!("{CONTENT_WIDTH} pb-20 pt-10 lg:pt-16")) {
                    a class="text-[0.75rem] font-semibold text-muted no-underline hover:text-ink" href="/stacks" { "← All stacks" }
                    p class=(format!("{EYEBROW} mb-3 mt-6 text-brand-bright")) { "Managed definition" }
                    h1 class="m-0 font-display text-[clamp(2.2rem,5vw,4rem)] font-normal tracking-[-0.04em]" { "Delete stack" }
                    p class="mt-4 max-w-[44rem] leading-[1.7] text-muted" {
                        (stack.name) " · " (stack.kind.as_str()) " · " (provider_name)
                    }
                    @if let Some(message) = error {
                        div class="mb-6 mt-6 border-l-[3px] border-danger bg-danger/5 px-4 py-3.5 text-sm leading-normal text-danger" role="alert" { (message) }
                    }
                    form method="post" action=(format!("/stacks/{}/delete", stack.id)) class="mt-8 grid max-w-[44rem] grid-cols-1 gap-5 rounded-[0.85rem] border border-line bg-surface/45 p-6" {
                        input type="hidden" name="csrf_token" value=(csrf_token);
                        p class="m-0 text-sm leading-relaxed text-muted" {
                            "This permanently removes " strong class="text-ink" { (stack.name) } " from the catalog. Type the stack name exactly to confirm."
                        }
                        label class="flex flex-col gap-2 text-[0.8rem] font-bold" {
                            "Stack name"
                            input class=(FIELD) name="confirm_name" type="text" autocomplete="off" required;
                        }
                        div class="flex flex-wrap items-center gap-3" {
                            button class="rounded-[0.6rem] border border-danger/40 bg-transparent px-4 py-3 font-bold text-danger" type="submit" { "Delete stack" }
                            a class="text-[0.8rem] font-semibold text-muted no-underline hover:text-ink" href="/stacks" { "Cancel" }
                        }
                    }
                }
            }
        },
    )
}

pub fn stack_detail(
    base_url: &str,
    username: &str,
    csrf_token: &str,
    provider: &ProviderConnection,
    stack_name: &str,
    result: Result<StackGroup, String>,
) -> Markup {
    page(
        "Stack · Hakobi",
        html! {
            div class="min-h-screen lg:pl-60" {
                (sidebar("stacks", base_url, username, csrf_token))
                header class="lg:hidden border-b border-line bg-paper/95" {
                    div class=(format!("{CONTENT_WIDTH} flex h-[5.5rem] items-center justify-between gap-4")) {
                        (brand("Hakobi dashboard"))
                        div class="flex items-center gap-4" {
                            a class="text-[0.8rem] font-semibold text-muted no-underline hover:text-ink" href="/stacks" { "Stacks" }
                            span class="text-sm text-muted max-sm:hidden" { (username) }
                            form method="post" action="/logout" class="m-0" {
                                input type="hidden" name="csrf_token" value=(csrf_token);
                                button class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.8rem] font-semibold text-ink" type="submit" { "Sign out" }
                            }
                        }
                    }
                }
                main class=(format!("{CONTENT_WIDTH} pb-20 pt-10 lg:pt-16")) {
                    a class="text-[0.75rem] font-semibold text-muted no-underline hover:text-ink" href="/stacks" { "← All stacks" }
                    p class=(format!("{EYEBROW} mb-3 mt-6 text-brand-bright")) { "Stack" }
                    h1 class="m-0 font-display text-[clamp(2.2rem,5vw,4rem)] font-normal tracking-[-0.04em]" { (stack_name) }
                    p class="mt-3 text-sm text-muted" { (provider.name) " · " (provider.endpoint) }
                    (delete_container_dialog(csrf_token))
                    @match result {
                        Ok(stack) => {
                            div class="mt-8 grid gap-5" {
                                @for service in &stack.services {
                                    section class="rounded-[0.85rem] border border-line bg-surface/45 p-5" {
                                        div class="flex flex-col justify-between gap-2 sm:flex-row sm:items-start" {
                                            div {
                                                h2 class="m-0 text-[1.05rem] font-bold" { (service.name) }
                                                p class="mb-0 mt-1 text-xs text-muted" {
                                                    (service.containers.len()) " container" @if service.containers.len() != 1 { "s" }
                                                }
                                            }
                                            @if let Some(container) = service.containers.first() {
                                                span class="font-mono text-[0.7rem] text-muted" { (container.image) }
                                            }
                                        }
                                        div class="mt-4 overflow-x-auto" {
                                            table class="w-full border-collapse text-left text-xs" {
                                                thead class="text-[0.6rem] uppercase tracking-[0.1em] text-muted" {
                                                    tr {
                                                        th class="pb-2 pr-3 font-semibold" { "Container" }
                                                        th class="pb-2 pr-3 font-semibold" { "State" }
                                                        th class="pb-2 pr-3 font-semibold" { "Health" }
                                                        th class="pb-2 pr-3 font-semibold" { "Ports" }
                                                        th class="pb-2 font-semibold" { "Actions" }
                                                    }
                                                }
                                                tbody {
                                                    @for container in &service.containers {
                                                        tr class="border-t border-line" {
                                                            td class="py-2 pr-3" {
                                                                span class="block font-semibold" { (container.name) }
                                                                span class="font-mono text-[0.65rem] text-muted" { (container.id.chars().take(12).collect::<String>()) }
                                                            }
                                                            td class="py-2 pr-3" {
                                                                span class="rounded-full bg-brand-bright/10 px-2.5 py-1 text-[0.65rem] font-semibold text-brand" { (container.state) }
                                                            }
                                                            td class="py-2 pr-3 text-muted" { (container.health.as_deref().unwrap_or("—")) }
                                                            td class="py-2 font-mono text-muted" { (container_ports(container)) }
                                                            td class="py-2" {
                                                                @if provider.kind == "docker" {
                                                                    (container_actions(&provider.id, container, csrf_token))
                                                                } @else {
                                                                    span class="text-xs text-muted" { "—" }
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
                        },
                        Err(error) => {
                            div class="mt-8 rounded-[0.85rem] border border-danger/30 bg-danger/5 p-5 text-sm text-danger" {
                                strong class="block" { "Provider read failed" }
                                span class="mt-1 block break-words" { (error) }
                            }
                        }
                    }
                }
            }
        },
    )
}
