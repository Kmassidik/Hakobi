//! Provider page template.

use maud::{html, Markup};

use hakobi::catalog::ProviderConnection;

use super::{brand, page, sidebar, CONTENT_WIDTH, EYEBROW, FIELD};

pub fn providers(
    base_url: &str,
    username: &str,
    csrf_token: &str,
    providers: &[ProviderConnection],
    notice: Option<&str>,
) -> Markup {
    page(
        "Providers · Hakobi",
        html! {
            div class="min-h-screen lg:pl-64" {
                (sidebar("providers", base_url, username, csrf_token))
                header class="lg:hidden border-b border-line bg-paper/95" {
                    div class=(format!("{CONTENT_WIDTH} flex h-[5.5rem] items-center justify-between gap-4")) {
                        (brand("Hakobi dashboard"))
                        div class="flex items-center gap-4" {
                            a class="text-[0.8rem] font-semibold text-muted no-underline hover:text-ink" href="/" { "Back to overview" }
                            span class="text-sm text-muted max-sm:hidden" { (username) }
                            form method="post" action="/logout" class="m-0" {
                                input type="hidden" name="csrf_token" value=(csrf_token);
                                button class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.8rem] font-semibold text-ink" type="submit" { "Sign out" }
                            }
                        }
                    }
                }
                main class=(format!("{CONTENT_WIDTH} pb-20 pt-10 lg:pt-16")) {
                    div class="mb-8" {
                        p class=(format!("{EYEBROW} mb-3 text-brand-bright")) { "Connections" }
                        h1 class="m-0 font-display text-[clamp(2.2rem,5vw,4rem)] font-normal tracking-[-0.04em]" { "Providers" }
                        p class="mt-4 max-w-[44rem] leading-[1.7] text-muted" {
                            "Connect Hakobi to the systems that run your software. Docker is available now; more providers arrive as separate connectors."
                        }
                    }
                    @if let Some(message) = notice {
                        p class="mb-6 rounded-[0.6rem] border border-line bg-surface/60 px-4 py-3 text-sm" role="status" { (message) }
                    }
                    section class="mb-8 rounded-[0.85rem] border border-line bg-surface/45 p-6" aria-labelledby="add-provider-title" {
                        h2 id="add-provider-title" class="mb-4 mt-0 font-display text-[1.6rem] font-normal" { "Add Docker provider" }
                        form method="post" action="/providers" class="grid gap-4 md:grid-cols-2" {
                            input type="hidden" name="csrf_token" value=(csrf_token);
                            label class="flex flex-col gap-2 text-[0.8rem] font-bold" {
                                "Name"
                                input class=(FIELD) name="name" type="text" value="local" maxlength="64" required;
                            }
                            label class="flex flex-col gap-2 text-[0.8rem] font-bold" {
                                "Endpoint"
                                input class=(FIELD) name="endpoint" type="text" value="unix:///var/run/docker.sock" maxlength="512" required;
                            }
                            label class="flex flex-col gap-2 text-[0.8rem] font-bold md:col-span-2" {
                                "Secret (optional)"
                                input class=(FIELD) name="secret" type="password" autocomplete="off" maxlength="4096";
                            }
                            div class="md:col-span-2" {
                                button class="rounded-[0.6rem] bg-brand px-4 py-3 font-bold text-white transition hover:bg-[#0f4130]" type="submit" {
                                    i class="fa-solid fa-plus mr-2" aria-hidden="true" {}
                                    "Add provider"
                                }
                            }
                        }
                        p class="mb-0 mt-4 text-xs leading-relaxed text-muted" {
                            "Use a Unix socket for a local daemon or tcp://host:port for an explicitly reachable endpoint. Credentials are encrypted at rest."
                        }
                    }
                    @if providers.is_empty() {
                        div class="rounded-[0.85rem] border border-dashed border-line p-8 text-center text-muted" {
                            img src="/assets/art/illustrations/empty-providers.webp" alt="" aria-hidden="true" class="empty-art";
                            p class="m-0" { "No providers connected yet." }
                        }
                    } @else {
                        div class="grid gap-4" {
                            @for provider in providers {
                                article class="rounded-[0.85rem] border border-line bg-surface/45 p-6" {
                                    div class="flex flex-col justify-between gap-3 sm:flex-row sm:items-start" {
                                        div {
                                            h2 class="m-0 text-[1.1rem] font-bold" { (provider.name) }
                                            p class="mb-0 mt-1 break-all font-mono text-xs text-muted" { (provider.endpoint) }
                                        }
                                        span class=(if provider.enabled { "inline-flex w-fit items-center gap-2 rounded-full bg-brand-bright/10 px-3 py-1 text-xs font-semibold text-brand" } else { "inline-flex w-fit items-center gap-2 rounded-full bg-line/40 px-3 py-1 text-xs font-semibold text-muted" }) {
                                            span class=(if provider.enabled { "size-1.5 rounded-full bg-brand-bright" } else { "size-1.5 rounded-full bg-muted" }) aria-hidden="true" {}
                                            (if provider.enabled { "Enabled" } else { "Disabled" })
                                        }
                                    }
                                    div class="mt-4 grid gap-2 text-xs text-muted sm:grid-cols-2" {
                                        p class="m-0" {
                                            "Kind: " span class="font-semibold text-ink" { (provider.kind) }
                                        }
                                        p class="m-0" {
                                            "Last check: " span class="font-semibold text-ink" { (provider.last_checked_at.as_deref().unwrap_or("never")) }
                                        }
                                        @if let Some(error) = &provider.last_error {
                                            p class="m-0 text-danger sm:col-span-2" { "Last error: " (error) }
                                        }
                                        @if !provider.capabilities.is_empty() {
                                            p class="m-0 sm:col-span-2" { "Capabilities: " (provider.capabilities.join(", ")) }
                                        }
                                    }
                                    div class="mt-5 flex flex-wrap gap-2" {
                                        form method="post" action=(format!("/providers/{}/test", provider.id)) class="m-0" {
                                            input type="hidden" name="csrf_token" value=(csrf_token);
                                            button class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold" type="submit" {
                                                i class="fa-solid fa-plug mr-2" aria-hidden="true" {}
                                                "Test"
                                            }
                                        }
                                        form method="post" action=(format!("/providers/{}/toggle", provider.id)) class="m-0" {
                                            input type="hidden" name="csrf_token" value=(csrf_token);
                                            button class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold" type="submit" {
                                                (if provider.enabled { "Disable" } else { "Enable" })
                                            }
                                        }
                                        form method="post" action=(format!("/providers/{}/delete", provider.id)) class="m-0" {
                                            input type="hidden" name="csrf_token" value=(csrf_token);
                                            button class="rounded-lg border border-danger/40 bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-danger" type="submit" { "Delete" }
                                        }
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
