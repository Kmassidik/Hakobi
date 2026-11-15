//! Docker image, network, and volume workbench panels.

use std::collections::HashMap;

use maud::{html, Markup};

use hakobi::{
    catalog::ProviderConnection,
    providers::{ContainerInventory, DockerSnapshot, ImageRow, NetworkRow, VolumeRow},
};

use super::{pagination_controls, FIELD};
use crate::web::pagination::{page_number, paginate};

pub type SnapshotResults = HashMap<String, Result<DockerSnapshot, String>>;

pub(crate) fn docker_tabs() -> Markup {
    html! {
        nav class="mt-6 flex flex-wrap gap-2" aria-label="Docker resources" {
            @for (key, label) in [("containers", "Containers"), ("images", "Images"), ("networks", "Networks"), ("volumes", "Volumes")] {
                button type="button" class="rounded-lg border border-line bg-surface px-3.5 py-2 text-[0.78rem] font-semibold text-ink" data-docker-tab=(key) aria-selected=(key == "containers") { (label) }
            }
        }
    }
}

pub(crate) fn docker_resource_panels(
    groups: &[(ProviderConnection, Result<ContainerInventory, String>)],
    snapshots: &SnapshotResults,
    csrf_token: &str,
    query: &HashMap<String, String>,
) -> Markup {
    html! {
        @for (provider, _) in groups {
            @if let Some(snapshot) = snapshots.get(&provider.id) {
                (provider_resources(provider, snapshot, csrf_token, query))
            }
        }
        (delete_resource_dialog(csrf_token))
    }
}

fn provider_resources(
    provider: &ProviderConnection,
    snapshot: &Result<DockerSnapshot, String>,
    csrf_token: &str,
    query: &HashMap<String, String>,
) -> Markup {
    html! {
        @match snapshot {
            Ok(snapshot) => {
                (images_panel(provider, snapshot, csrf_token, query))
                (networks_panel(provider, snapshot, csrf_token, query))
                (volumes_panel(provider, snapshot, csrf_token, query))
            }
            Err(error) => p class="mt-6 text-sm text-danger" { "Docker resources unavailable: " (error) }
        }
    }
}

fn panel_header(provider: &ProviderConnection, title: &str, count: usize) -> Markup {
    html! {
        div class="mb-3 flex items-end justify-between gap-3" {
            h2 class="m-0 text-[1.1rem] font-bold" { (title) }
            span class="text-xs text-muted" { (provider.name) " · " (count) }
        }
    }
}

fn images_panel(
    provider: &ProviderConnection,
    snapshot: &DockerSnapshot,
    csrf_token: &str,
    query: &HashMap<String, String>,
) -> Markup {
    let page = paginate(&snapshot.images, page_number(query, "images"), 25);
    html! {
        section data-docker-panel="images" hidden class="mt-6" {
            (panel_header(provider, "Images", page.total))
            div class="overflow-x-auto rounded-[0.85rem] border border-line" {
                table class="w-full border-collapse text-left text-sm" {
                    (resource_header(&["Repository:Tag", "Image ID", "Size", "Actions"]))
                    tbody {
                        @for image in page.items { (image_row(provider, image, csrf_token)) }
                    }
                }
            }
            (pagination_controls("/containers", query, "images", &page, Some("images")))
        }
    }
}

fn image_row(provider: &ProviderConnection, image: &ImageRow, csrf_token: &str) -> Markup {
    let name = image.tags.first().map_or(image.id.as_str(), String::as_str);
    let tags = if image.tags.is_empty() {
        "<none>".to_owned()
    } else {
        image.tags.join(", ")
    };
    html! {
        tr class="border-t border-line" {
            td class="px-4 py-3 font-mono text-xs font-semibold" { (tags) }
            td class="px-4 py-3 font-mono text-xs text-muted" { (image.id.chars().take(19).collect::<String>()) }
            td class="px-4 py-3 text-xs text-muted" { (format!("{:.1} MiB", image.size_bytes as f64 / 1_048_576.0)) }
            td class="px-4 py-3 text-right" { (delete_button(provider, "/docker/images/delete", name, csrf_token)) }
        }
    }
}

fn networks_panel(
    provider: &ProviderConnection,
    snapshot: &DockerSnapshot,
    csrf_token: &str,
    query: &HashMap<String, String>,
) -> Markup {
    let page = paginate(&snapshot.networks, page_number(query, "networks"), 25);
    html! {
        section data-docker-panel="networks" hidden class="mt-6" {
            (panel_header(provider, "Networks", page.total))
            (create_form(provider, "/docker/networks/create", "network-name", csrf_token))
            div class="mt-4 overflow-x-auto rounded-[0.85rem] border border-line" {
                table class="w-full border-collapse text-left text-sm" {
                    (resource_header(&["Name", "Driver", "Scope", "Actions"]))
                    tbody {
                        @for network in page.items { (network_row(provider, network, csrf_token)) }
                    }
                }
            }
            (pagination_controls("/containers", query, "networks", &page, Some("networks")))
        }
    }
}

fn network_row(provider: &ProviderConnection, network: &NetworkRow, csrf_token: &str) -> Markup {
    let built_in = matches!(network.name.as_str(), "bridge" | "host" | "none");
    html! {
        tr class="border-t border-line" {
            td class="px-4 py-3 font-mono text-xs font-semibold" { (network.name) }
            td class="px-4 py-3 text-xs text-muted" { (network.driver) }
            td class="px-4 py-3 text-xs text-muted" { (network.scope) }
            td class="px-4 py-3 text-right" {
                @if built_in { span class="text-xs text-muted" { "Built-in" } } @else {
                    (delete_button(provider, "/docker/networks/delete", &network.name, csrf_token))
                }
            }
        }
    }
}

fn volumes_panel(
    provider: &ProviderConnection,
    snapshot: &DockerSnapshot,
    csrf_token: &str,
    query: &HashMap<String, String>,
) -> Markup {
    let page = paginate(&snapshot.volumes, page_number(query, "volumes"), 25);
    html! {
        section data-docker-panel="volumes" hidden class="mt-6" {
            (panel_header(provider, "Volumes", page.total))
            (create_form(provider, "/docker/volumes/create", "volume-name", csrf_token))
            div class="mt-4 overflow-x-auto rounded-[0.85rem] border border-line" {
                table class="w-full border-collapse text-left text-sm" {
                    (resource_header(&["Name", "Driver", "Mountpoint", "Actions"]))
                    tbody {
                        @for volume in page.items { (volume_row(provider, volume, csrf_token)) }
                    }
                }
            }
            (pagination_controls("/containers", query, "volumes", &page, Some("volumes")))
        }
    }
}

fn volume_row(provider: &ProviderConnection, volume: &VolumeRow, csrf_token: &str) -> Markup {
    html! {
        tr class="border-t border-line" {
            td class="px-4 py-3 font-mono text-xs font-semibold" { (volume.name) }
            td class="px-4 py-3 text-xs text-muted" { (volume.driver) }
            td class="max-w-[24rem] break-all px-4 py-3 font-mono text-[0.68rem] text-muted" { (volume.mountpoint) }
            td class="px-4 py-3 text-right" { (delete_button(provider, "/docker/volumes/delete", &volume.name, csrf_token)) }
        }
    }
}

fn resource_header(labels: &[&str]) -> Markup {
    html! {
        thead class="bg-surface/60 text-[0.65rem] uppercase tracking-[0.12em] text-muted" {
            tr { @for label in labels { th class="px-4 py-3 font-semibold" { (label) } } }
        }
    }
}

fn create_form(
    provider: &ProviderConnection,
    action: &str,
    placeholder: &str,
    csrf_token: &str,
) -> Markup {
    html! {
        form method="post" action=(action) class="flex flex-wrap gap-2" {
            input type="hidden" name="csrf_token" value=(csrf_token);
            input type="hidden" name="provider_id" value=(provider.id);
            input class=(format!("{FIELD} max-w-xs") ) name="name" placeholder=(placeholder) required autocomplete="off";
            button class="rounded-lg bg-brand px-3.5 py-2 text-[0.78rem] font-semibold text-white" type="submit" { "Create" }
        }
    }
}

fn delete_button(
    provider: &ProviderConnection,
    action: &str,
    name: &str,
    _csrf_token: &str,
) -> Markup {
    html! {
        button type="button" class="rounded-md border border-danger/40 px-2.5 py-1.5 text-[0.68rem] font-semibold text-danger" data-dialog-open="delete-docker-resource" data-delete-action=(action) data-delete-name=(name) data-provider-id=(provider.id) {
            "Delete"
        }
    }
}

fn delete_resource_dialog(csrf_token: &str) -> Markup {
    html! {
        dialog id="delete-docker-resource" class="w-[90%] max-w-[34rem] rounded-[0.85rem] border border-line bg-surface p-6 text-ink" {
            form method="post" class="m-0" {
                input type="hidden" name="csrf_token" value=(csrf_token);
                input type="hidden" name="provider_id" data-resource-provider;
                input type="hidden" name="name" data-resource-name;
                h2 class="m-0 text-[1.3rem] font-bold" { "Delete Docker resource?" }
                p class="mt-3 text-sm text-muted" { "Delete " strong data-confirm-name { "this resource" } "? Docker rejects resources still in use." }
                label class="mt-4 grid gap-2 text-sm font-semibold" { "Type the exact name to confirm"
                    input class=(FIELD) name="confirm_name" data-confirm-input required autocomplete="off";
                }
                div class="mt-5 flex gap-2" {
                    button type="button" class="rounded-lg border border-line px-3.5 py-2 text-sm" data-dialog-close { "Cancel" }
                    button type="submit" class="rounded-lg border border-danger/40 px-3.5 py-2 text-sm font-semibold text-danger" data-confirm-submit disabled { "Delete" }
                }
            }
        }
    }
}
