//! Infrastructure inventory template.

use maud::{html, Markup};

use hakobi::catalog::ProviderConnection;
use hakobi::providers::DockerSnapshot;

use super::containers::mib;
use super::{brand, page, sidebar, CONTENT_WIDTH, EYEBROW};

pub fn infrastructure(
    base_url: &str,
    username: &str,
    csrf_token: &str,
    groups: &[(ProviderConnection, Result<DockerSnapshot, String>)],
) -> Markup {
    page(
        "Infrastructure · Hakobi",
        html! {
            div class="min-h-screen lg:pl-64" {
                (sidebar("infrastructure", base_url, username, csrf_token))
                header class="lg:hidden border-b border-line bg-paper/95" {
                    div class=(format!("{CONTENT_WIDTH} flex h-[5.5rem] items-center justify-between gap-4")) {
                        (brand("Hakobi dashboard"))
                        nav class="flex items-center gap-1 text-[0.8rem] font-semibold max-md:hidden" aria-label="Primary" {
                            a class="rounded-lg px-3 py-2 no-underline transition hover:bg-surface" href="/" { "Overview" }
                            a class="rounded-lg px-3 py-2 no-underline transition hover:bg-surface" href="/providers" { "Providers" }
                            a class="rounded-lg px-3 py-2 no-underline transition hover:bg-surface" href="/stacks" { "Stacks" }
                            a class="rounded-lg px-3 py-2 no-underline transition hover:bg-surface" href="/containers" { "Containers" }
                        }
                        div class="flex items-center gap-4" {
                            span class="text-sm text-muted max-sm:hidden" { (username) }
                            form method="post" action="/logout" class="m-0" {
                                input type="hidden" name="csrf_token" value=(csrf_token);
                                button class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.8rem] font-semibold text-ink" type="submit" { "Sign out" }
                            }
                        }
                    }
                }
                main class=(format!("{CONTENT_WIDTH} pb-20 pt-10 lg:pt-16")) {
                    p class=(format!("{EYEBROW} mb-3 text-brand-bright")) { "Docker inventory" }
                    h1 class="m-0 font-display text-[clamp(2.2rem,5vw,4rem)] font-normal tracking-[-0.04em]" { "Infrastructure" }
                    p class="mt-4 max-w-[44rem] leading-[1.7] text-muted" {
                        "Host, images, volumes, and networks from enabled Docker providers. Read-only and scoped to each connection."
                    }
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
                        section class="mt-10" {
                            div class="mb-4 flex flex-col gap-1 sm:flex-row sm:items-end sm:justify-between" {
                                h2 class="m-0 text-[1.25rem] font-bold" { (provider.name) }
                                span class="break-all font-mono text-xs text-muted" { (provider.endpoint) }
                            }
                            @match result {
                                Ok(snapshot) => {
                                    div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-4" {
                                        article class="rounded-[0.85rem] border border-line bg-surface/45 p-5" {
                                            p class=(format!("{EYEBROW} mb-2 text-[0.6rem] text-muted")) { "Host" }
                                            p class="m-0 text-lg font-bold" { (snapshot.host.name) }
                                            p class="m-0 mt-1 text-xs text-muted" { "Docker " (snapshot.host.server_version) }
                                        }
                                        article class="rounded-[0.85rem] border border-line bg-surface/45 p-5" {
                                            p class=(format!("{EYEBROW} mb-2 text-[0.6rem] text-muted")) { "Containers" }
                                            p class="m-0 text-lg font-bold" { (snapshot.host.containers) }
                                            p class="m-0 mt-1 text-xs text-muted" { (snapshot.host.running) " running" }
                                        }
                                        article class="rounded-[0.85rem] border border-line bg-surface/45 p-5" {
                                            p class=(format!("{EYEBROW} mb-2 text-[0.6rem] text-muted")) { "Images" }
                                            p class="m-0 text-lg font-bold" { (snapshot.host.images) }
                                            p class="m-0 mt-1 text-xs text-muted" { "local cache" }
                                        }
                                        article class="rounded-[0.85rem] border border-line bg-surface/45 p-5" {
                                            p class=(format!("{EYEBROW} mb-2 text-[0.6rem] text-muted")) { "Platform" }
                                            p class="m-0 text-lg font-bold" { (snapshot.host.architecture) }
                                            p class="m-0 mt-1 text-xs text-muted" { (snapshot.host.operating_system) " · " (snapshot.host.cpus) " CPUs" }
                                        }
                                    }
                                    div class="mt-5 grid gap-5 lg:grid-cols-2" {
                                        section class="rounded-[0.85rem] border border-line p-5" {
                                            h3 class="mb-3 mt-0 text-[1rem] font-bold" { "Images" }
                                            @if snapshot.images.is_empty() {
                                                p class="m-0 text-sm text-muted" { "No images." }
                                            } @else {
                                                div class="overflow-x-auto" {
                                                    table class="w-full border-collapse text-left text-xs" {
                                                        thead class="text-[0.6rem] uppercase tracking-[0.1em] text-muted" {
                                                            tr { th class="pb-2 font-semibold" { "Tag" } th class="pb-2 font-semibold" { "ID" } th class="pb-2 text-right font-semibold" { "Size" } }
                                                        }
                                                        tbody {
                                                            @for image in &snapshot.images {
                                                                tr class="border-t border-line" {
                                                                    td class="max-w-[16rem] truncate py-2 pr-3 font-mono" { (if image.tags.is_empty() { "untagged".to_owned() } else { image.tags.join(", ") }) }
                                                                    td class="py-2 pr-3 font-mono text-muted" { (image.id.chars().take(12).collect::<String>()) }
                                                                    td class="py-2 text-right text-muted" { (mib(image.size_bytes)) }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        section class="rounded-[0.85rem] border border-line p-5" {
                                            h3 class="mb-3 mt-0 text-[1rem] font-bold" { "Volumes" }
                                            @if snapshot.volumes.is_empty() {
                                                p class="m-0 text-sm text-muted" { "No volumes." }
                                            } @else {
                                                div class="overflow-x-auto" {
                                                    table class="w-full border-collapse text-left text-xs" {
                                                        thead class="text-[0.6rem] uppercase tracking-[0.1em] text-muted" {
                                                            tr { th class="pb-2 font-semibold" { "Name" } th class="pb-2 font-semibold" { "Driver" } th class="pb-2 font-semibold" { "Mountpoint" } }
                                                        }
                                                        tbody {
                                                            @for volume in &snapshot.volumes {
                                                                tr class="border-t border-line" {
                                                                    td class="py-2 pr-3 font-semibold" { (volume.name) }
                                                                    td class="py-2 pr-3 text-muted" { (volume.driver) }
                                                                    td class="max-w-[18rem] truncate py-2 font-mono text-muted" { (volume.mountpoint) }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    section class="mt-5 rounded-[0.85rem] border border-line p-5" {
                                        h3 class="mb-3 mt-0 text-[1rem] font-bold" { "Networks" }
                                        @if snapshot.networks.is_empty() {
                                            p class="m-0 text-sm text-muted" { "No networks." }
                                        } @else {
                                            div class="overflow-x-auto" {
                                                table class="w-full border-collapse text-left text-xs" {
                                                    thead class="text-[0.6rem] uppercase tracking-[0.1em] text-muted" {
                                                        tr { th class="pb-2 font-semibold" { "Name" } th class="pb-2 font-semibold" { "Driver" } th class="pb-2 font-semibold" { "Scope" } }
                                                    }
                                                    tbody {
                                                        @for network in &snapshot.networks {
                                                            tr class="border-t border-line" {
                                                                td class="py-2 pr-3 font-semibold" { (network.name) }
                                                                td class="py-2 pr-3 text-muted" { (network.driver) }
                                                                td class="py-2 text-muted" { (network.scope) }
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
                                        strong class="block" { "Provider inventory failed" }
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
