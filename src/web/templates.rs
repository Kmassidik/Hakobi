use maud::{html, Markup, DOCTYPE};

use hakobi::{
    catalog::ProviderConnection,
    install::{Readiness, ToolStatus},
    providers::{ContainerInventory, ContainerRow, DockerSnapshot, StackGroup},
    stacks::{StackDefinition, StackKind},
};

const PAGE_BODY: &str = "min-w-80 bg-paper font-sans text-ink antialiased";
const BRAND: &str =
    "inline-flex items-center gap-3 text-[1.12rem] font-bold tracking-[-0.03em] no-underline";
const BRAND_MARK: &str =
    "grid size-9 place-items-center rounded-[0.65rem] bg-lime font-extrabold text-brand";
const CONTENT_WIDTH: &str = "mx-auto w-[90%] max-w-[78rem]";
const EYEBROW: &str = "eyebrow";
const FIELD: &str = "w-full rounded-[0.6rem] border border-line bg-surface px-4 py-3.5 text-ink outline-none transition focus:border-brand-bright focus:ring-[3px] focus:ring-brand-bright/15";

pub(super) fn page(title: &str, content: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                meta name="color-scheme" content="light";
                title { (title) }
                link rel="stylesheet" href=(super::handlers::asset_url("app.css"));
                link rel="stylesheet" href=(super::handlers::asset_url("vendor/fontawesome/css/all.min.css"));
                script src=(super::handlers::asset_url("htmx.min.js")) defer {}
                script src=(super::handlers::asset_url("hakobi.js")) defer {}
            }
            body class=(PAGE_BODY) {
                (content)
            }
        }
    }
}

fn brand(home_label: &str) -> Markup {
    html! {
        a class=(BRAND) href="/" aria-label=(home_label) {
            span class=(BRAND_MARK) aria-hidden="true" { "運" }
            span { "Hakobi" }
        }
    }
}

fn nav_item(active: &str, key: &str, href: &str, icon: &str, label: &str) -> Markup {
    let class = if active == key {
        "flex items-center gap-3 rounded-xl bg-brand px-3.5 py-2.5 text-sm font-semibold text-white no-underline shadow-sm"
    } else {
        "flex items-center gap-3 rounded-xl px-3.5 py-2.5 text-sm font-semibold text-muted no-underline transition hover:bg-surface hover:text-ink"
    };
    html! {
        a class=(class) href=(href) aria-current=(if active == key { "page" } else { "false" }) {
            i class=(format!("fa-solid {icon} w-4 text-center")) aria-hidden="true" {}
            span { (label) }
        }
    }
}

fn mobile_nav_item(active: &str, key: &str, href: &str, icon: &str, label: &str) -> Markup {
    let class = if active == key {
        "flex min-w-0 flex-col items-center gap-1 rounded-lg bg-brand/10 px-1 py-2 text-[0.62rem] font-bold text-brand no-underline"
    } else {
        "flex min-w-0 flex-col items-center gap-1 rounded-lg px-1 py-2 text-[0.62rem] font-semibold text-muted no-underline transition hover:bg-surface hover:text-ink"
    };
    html! {
        a class=(class) href=(href) aria-current=(if active == key { "page" } else { "false" }) {
            i class=(format!("fa-solid {icon} text-[0.95rem]")) aria-hidden="true" {}
            span class="w-full truncate text-center" { (label) }
        }
    }
}

fn sidebar(active: &str, base_url: &str, username: &str, csrf_token: &str) -> Markup {
    html! {
        aside class="fixed inset-y-0 left-0 z-40 hidden w-64 flex-col border-r border-line bg-[#faf9f4] px-5 py-6 lg:flex" aria-label="Sidebar" {
            div class="px-1" { (brand("Hakobi dashboard")) }
            nav class="mt-10 flex flex-col gap-1" aria-label="Primary" {
                (nav_item(active, "overview", "/", "fa-gauge-high", "Overview"))
                (nav_item(active, "stacks", "/stacks", "fa-layer-group", "Stacks"))
                (nav_item(active, "containers", "/containers", "fa-cubes", "Containers"))
                (nav_item(active, "infrastructure", "/infrastructure", "fa-server", "Infrastructure"))
                (nav_item(active, "providers", "/providers", "fa-plug", "Providers"))
            }
            div class="mt-auto border-t border-line pt-5" {
                (status_badge())
                div class="mt-4 flex items-center gap-3 px-1" {
                    span class="grid size-9 shrink-0 place-items-center rounded-full bg-brand text-xs font-bold uppercase text-white" aria-hidden="true" {
                        (username.chars().next().unwrap_or('H'))
                    }
                    div class="min-w-0" {
                        p class="m-0 truncate text-sm font-semibold" { (username) }
                        p class="m-0 truncate text-[0.68rem] text-muted" { (base_url) }
                    }
                }
                form method="post" action="/logout" class="m-0 mt-4" {
                    input type="hidden" name="csrf_token" value=(csrf_token);
                    button class="flex w-full items-center gap-3 rounded-xl border border-line bg-transparent px-3.5 py-2.5 text-left text-sm font-semibold text-ink transition hover:bg-surface" type="submit" {
                        i class="fa-solid fa-arrow-right-from-bracket w-4 text-center" aria-hidden="true" {}
                        "Sign out"
                    }
                }
            }
        }
        nav class="fixed inset-x-0 bottom-0 z-40 border-t border-line bg-paper/95 px-2 pb-2 pt-1.5 backdrop-blur lg:hidden" aria-label="Mobile primary" {
            div class="mx-auto grid max-w-md grid-cols-5 gap-0.5" {
                (mobile_nav_item(active, "overview", "/", "fa-gauge-high", "Overview"))
                (mobile_nav_item(active, "stacks", "/stacks", "fa-layer-group", "Stacks"))
                (mobile_nav_item(active, "containers", "/containers", "fa-cubes", "Containers"))
                (mobile_nav_item(active, "infrastructure", "/infrastructure", "fa-server", "Infra"))
                (mobile_nav_item(active, "providers", "/providers", "fa-plug", "Providers"))
            }
        }
    }
}

pub(super) fn login(
    _base_url: &str,
    csrf_token: &str,
    error: Option<&str>,
    username: &str,
) -> Markup {
    page(
        "Sign in · Hakobi",
        html! {
            main class="login-page" {
                div class="login-shell" {
                    section class="login-brand" aria-labelledby="brand-title" {
                        div class="login-brand-top" {
                            (brand("Hakobi home"))
                            span class="login-badge" { "Self-hosted · Private by design" }
                        }
                        div class="login-copy" {
                            p class="login-kicker" { "Self-hosted delivery" }
                            h1 id="brand-title" class="login-title" { "Ship calmly." }
                            p class="login-subtitle" {
                                "Hakobi carries your code from commit to production, without the YAML maze."
                            }
                        }
                        img src="/assets/art/mascot/hako-carry.webp" alt="" aria-hidden="true" class="login-illustration";
                    }

                    section class="login-form" aria-labelledby="login-title" {
                        div class="login-form-inner" {
                            div class="login-mobile-brand" { (brand("Hakobi home")) }
                            p class="login-overline" { "Control plane access" }
                            h2 id="login-title" class="login-heading" { "Welcome back." }
                            p class="login-intro" { "Sign in to your Hakobi dashboard." }

                            @if let Some(message) = error {
                                p class="login-error" role="alert" { (message) }
                            }

                            form method="post" action="/login" class="login-fields" {
                                input type="hidden" name="csrf_token" value=(csrf_token);
                                label for="username" class="login-label" { "Username" }
                                div class="login-field" {
                                    i class="fa-solid fa-user login-field-icon" aria-hidden="true" {}
                                    input id="username" name="username" type="text" value=(username) placeholder="Your username" autocomplete="username" autocapitalize="none" spellcheck="false" maxlength="32" required autofocus class="login-input";
                                }
                                label for="password" class="login-label" { "Password" }
                                div class="login-field" {
                                    i class="fa-solid fa-lock login-field-icon" aria-hidden="true" {}
                                    input id="password" name="password" type="password" placeholder="Your password" autocomplete="current-password" maxlength="128" required class="login-input";
                                    button type="button" class="login-toggle" data-password-toggle aria-label="Show password" aria-pressed="false" {
                                        i class="fa-solid fa-eye" aria-hidden="true" {}
                                    }
                                }
                                button type="submit" class="login-submit" {
                                    span { "Sign in" }
                                    i class="fa-solid fa-arrow-right" aria-hidden="true" {}
                                }
                            }
                            p class="login-footer" { "Hakobi · Self-hosted control plane" }
                        }
                    }
                }
            }
        },
    )
}

pub(super) fn dashboard(
    base_url: &str,
    csrf_token: &str,
    tunnel_enabled: bool,
    username: &str,
    readiness: &Readiness,
) -> Markup {
    page(
        "Dashboard · Hakobi",
        html! {
            div class="min-h-screen lg:pl-64" {
                (sidebar("overview", base_url, username, csrf_token))
                header class="lg:hidden border-b border-line bg-paper/95" {
                    div class=(format!("{CONTENT_WIDTH} flex h-[5.5rem] items-center justify-between")) {
                        (brand("Hakobi dashboard"))
                        nav class="flex items-center gap-1 text-[0.8rem] font-semibold max-md:hidden" aria-label="Primary" {
                            a class="rounded-lg px-3 py-2 no-underline transition hover:bg-surface" href="/" { "Overview" }
                            a class="rounded-lg px-3 py-2 no-underline transition hover:bg-surface" href="/stacks" { "Stacks" }
                            a class="rounded-lg px-3 py-2 no-underline transition hover:bg-surface" href="/containers" { "Containers" }
                            a class="rounded-lg px-3 py-2 no-underline transition hover:bg-surface" href="/infrastructure" { "Infrastructure" }
                        }
                        div class="flex items-center gap-4" {
                            span class="text-sm text-muted max-sm:hidden" aria-label={"Signed in as " (username)} { (username) }
                            form method="post" action="/logout" class="m-0" {
                                input type="hidden" name="csrf_token" value=(csrf_token);
                                button class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.8rem] font-semibold text-ink transition hover:bg-surface" type="submit" {
                                    i class="fa-solid fa-arrow-right-from-bracket mr-2" aria-hidden="true" {}
                                    "Sign out"
                                }
                            }
                        }
                    }
                }

                main class=(format!("{CONTENT_WIDTH} pb-20 pt-10 lg:pt-16")) {
                    div class="mb-8 flex flex-col items-start gap-4 lg:flex-row lg:items-end lg:justify-between lg:gap-8" {
                        div {
                            p class=(format!("{EYEBROW} mb-3 text-brand-bright")) { "Overview" }
                            h1 class="m-0 font-display text-[clamp(2rem,4vw,3.3rem)] font-normal tracking-[-0.04em]" {
                                "Good to see you, " (username) "."
                            }
                        }
                        (status_badge())
                    }

                    section class="relative flex min-h-[23rem] items-center overflow-hidden rounded-[1.25rem] bg-brand p-8 text-white sm:p-12 lg:p-[clamp(2.5rem,6vw,5rem)]" aria-labelledby="hello-title" {
                        img src="/assets/art/hero-pipeline.webp" alt="" aria-hidden="true" class="hero-art pointer-events-none absolute inset-0 h-full w-full object-cover";
                        div aria-hidden="true" class="hero-scrim";
                        div class="hero-orbit" aria-hidden="true" {}
                        div class="relative z-10 max-w-[39rem]" {
                            p class=(format!("{EYEBROW} text-lime")) { "The first step is live" }
                            h2 id="hello-title" class="my-4 font-display text-[clamp(3.8rem,8vw,7rem)] font-normal leading-none tracking-[-0.06em]" {
                                "Hello, world."
                            }
                            p class="mt-6 max-w-[34rem] leading-[1.7] text-[#c3d2ca]" {
                                "Your self-hosted control plane is running. Authentication and secure delivery are in place—ready for whatever you ship next."
                            }
                        }
                        span class="absolute -bottom-14 -right-4 font-display text-[clamp(12rem,28vw,24rem)] leading-none text-white/5" aria-hidden="true" {
                            "運び"
                        }
                    }

                    section class="mt-5" aria-labelledby="readiness-title" {
                        div class="mb-3 flex flex-col gap-1 sm:flex-row sm:items-end sm:justify-between" {
                            h2 id="readiness-title" class="m-0 text-[1.2rem] font-bold" { "Host readiness" }
                            p class="m-0 text-xs text-muted" { "Detected locally. Nothing is installed automatically." }
                        }
                        div class="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-4" {
                            (readiness_card("Docker", "fa-solid fa-cube", &readiness.docker))
                            (readiness_card("Compose", "fa-solid fa-layer-group", &readiness.compose))
                            (readiness_card("kubectl", "fa-solid fa-dharmachakra", &readiness.kubectl))
                            (readiness_card("Nixpacks", "fa-solid fa-box", &readiness.nixpacks))
                        }
                    }

                    section class="mt-5 grid grid-cols-1 gap-5 md:grid-cols-3" aria-label="Instance details" {
                        article class="min-w-0 rounded-[0.85rem] border border-line bg-surface/45 p-6" {
                            p class=(format!("{EYEBROW} mb-3 flex items-center gap-2 text-[0.6rem] text-muted")) {
                                i class="fa-solid fa-globe" aria-hidden="true" {}
                                "Public address"
                            }
                            a class="block truncate text-[0.88rem] font-semibold no-underline" href=(base_url) { (base_url) }
                        }
                        article class="min-w-0 rounded-[0.85rem] border border-line bg-surface/45 p-6" {
                            p class=(format!("{EYEBROW} mb-3 flex items-center gap-2 text-[0.6rem] text-muted")) {
                                i class="fa-solid fa-cloud" aria-hidden="true" {}
                                "Delivery"
                            }
                            @if tunnel_enabled {
                                p class="m-0 text-[0.88rem] font-semibold" {
                                    "Cloudflare Tunnel"
                                    span class="text-xs font-normal text-muted" { " · outbound only" }
                                }
                            } @else {
                                p class="m-0 text-[0.88rem] font-semibold" {
                                    "Loopback only"
                                    span class="text-xs font-normal text-muted" { " · local development" }
                                }
                            }
                        }
                        article class="min-w-0 rounded-[0.85rem] border border-line bg-surface/45 p-6" {
                            p class=(format!("{EYEBROW} mb-3 flex items-center gap-2 text-[0.6rem] text-muted")) {
                                i class="fa-solid fa-shield-halved" aria-hidden="true" {}
                                "Session"
                            }
                            p class="m-0 text-[0.88rem] font-semibold" {
                                "Authenticated"
                                span class="text-xs font-normal text-muted" { " · 12 hour expiry" }
                            }
                        }
                    }
                    (install_dialog())
                }

                footer class=(format!("{CONTENT_WIDTH} flex flex-col gap-4 border-t border-line py-6 text-[0.65rem] tracking-[0.1em] text-muted sm:flex-row sm:justify-between")) {
                    span { "Hakobi / 運び" }
                    span { "Your infra. Your data." }
                }
            }
        },
    )
}

fn readiness_card(label: &str, icon: &str, status: &ToolStatus) -> Markup {
    html! {
        article class="rounded-[0.85rem] border border-line bg-surface/45 p-5" {
            div class="flex items-center gap-3" {
                span class="grid size-9 place-items-center rounded-full bg-brand-bright/10 text-brand" aria-hidden="true" {
                    i class=(format!("fa-solid {icon}")) {}
                }
                h3 class="m-0 text-[1rem] font-bold" { (label) }
            }
            @match status {
                ToolStatus::Ready { version } => {
                    p class="mb-0 mt-3 text-xs text-muted" {
                        "Ready · "
                        code class="font-mono text-ink" { (version) }
                    }
                },
                ToolStatus::Missing { install_command } => {
                    p class="mb-0 mt-3 text-xs text-muted" { "Missing" }
                    button type="button" class="mt-3 rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold text-ink transition hover:bg-surface" data-dialog-open="install-tool" data-install-title=(label) data-install-command=(install_command) {
                        "Install"
                    }
                },
            }
        }
    }
}

fn install_dialog() -> Markup {
    html! {
        dialog id="install-tool" class="w-[90%] max-w-[44rem] rounded-[0.85rem] border border-line bg-surface p-6 text-ink" aria-labelledby="install-tool-title" {
            form method="dialog" class="m-0" {
                div class="flex items-start justify-between gap-4" {
                    h2 id="install-tool-title" class="m-0 font-display text-[1.5rem] font-normal" data-install-title { "Install tooling" }
                    button type="submit" class="rounded-lg border border-line bg-transparent px-3 py-1 text-sm font-semibold text-ink" aria-label="Close dialog" { "Close" }
                }
                p class="mt-3 text-sm leading-relaxed text-muted" {
                    "Run this command on the host, then reload this page. Hakobi never executes installation commands."
                }
                pre class="mt-4 overflow-x-auto rounded-[0.6rem] border border-line bg-paper p-4" {
                    code class="font-mono text-xs" data-install-command {}
                }
                div class="mt-4 flex flex-wrap gap-2" {
                    button type="button" class="rounded-lg bg-brand px-3.5 py-2 text-[0.78rem] font-bold text-white" data-copy-command { "Copy command" }
                    button type="submit" class="rounded-lg border border-line bg-transparent px-3.5 py-2 text-[0.78rem] font-semibold" { "Done" }
                }
            }
        }
    }
}

pub(super) fn providers(
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

pub(super) fn containers(
    base_url: &str,
    username: &str,
    csrf_token: &str,
    groups: &[(ProviderConnection, Result<ContainerInventory, String>)],
    notice: Option<&str>,
) -> Markup {
    page(
        "Containers · Hakobi",
        html! {
            div class="min-h-screen lg:pl-64" {
                (sidebar("containers", base_url, username, csrf_token))
                header class="lg:hidden border-b border-line bg-paper/95" {
                    div class=(format!("{CONTENT_WIDTH} flex h-[5.5rem] items-center justify-between gap-4")) {
                        (brand("Hakobi dashboard"))
                        div class="flex items-center gap-4" {
                            a class="text-[0.8rem] font-semibold text-muted no-underline hover:text-ink" href="/" { "Back to overview" }
                            a class="text-[0.8rem] font-semibold text-muted no-underline hover:text-ink" href="/providers" { "Providers" }
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
                        section class="mt-8" {
                            div class="mb-3 flex flex-col gap-1 sm:flex-row sm:items-end sm:justify-between" {
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
                                    @if inventory.groups.is_empty() {
                                        p class="rounded-[0.85rem] border border-dashed border-line p-6 text-sm text-muted" { "No containers found." }
                                    } @else {
                                        @for group in &inventory.groups {
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
                                                        tbody {
                                                            @for container in &group.containers {
                                                                tr class="border-t border-line" {
                                                                    td class="px-4 py-3" {
                                                                        span class="block font-semibold" { (container.name) }
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

fn container_actions(provider: &str, container: &ContainerRow, csrf_token: &str) -> Markup {
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

fn delete_container_dialog(csrf_token: &str) -> Markup {
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

pub(super) struct ContainerFormValues {
    pub provider_id: String,
    pub image: String,
    pub name: String,
    pub container_port: String,
}

pub(super) fn container_form(
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
            div class="min-h-screen lg:pl-64" {
                (sidebar("containers", base_url, username, csrf_token))
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
                        form method="post" action="/containers/new" class="mt-8 grid max-w-[44rem] grid-cols-1 gap-5 rounded-[0.85rem] border border-line bg-surface/45 p-6" {
                            input type="hidden" name="csrf_token" value=(csrf_token);
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
                }
            }
        },
    )
}

fn mib(bytes: i64) -> String {
    format!("{:.1} MiB", bytes as f64 / 1_048_576.0)
}

fn container_ports(container: &ContainerRow) -> Markup {
    html! {
        @if container.ports.is_empty() {
            "—"
        } @else {
            @for port in &container.ports {
                span class="block whitespace-nowrap" {
                    @if let Some(host_port) = port.host_port {
                        (host_port) " → "
                    }
                    (port.container_port) "/" (port.protocol)
                }
            }
        }
    }
}

pub(super) fn stacks(
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
            div class="min-h-screen lg:pl-64" {
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
pub(super) struct StackFormValues {
    pub kind: String,
    pub provider_id: String,
    pub name: String,
    pub source: String,
    pub yaml: String,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn stack_form(
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
            div class="min-h-screen lg:pl-64" {
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

pub(super) fn stack_delete(
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
            div class="min-h-screen lg:pl-64" {
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

pub(super) fn stack_detail(
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
            div class="min-h-screen lg:pl-64" {
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

pub(super) fn infrastructure(
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

pub(super) fn status_badge() -> Markup {
    html! {
        span id="control-status" class="inline-flex items-center gap-2 whitespace-nowrap pb-2 text-xs text-muted" hx-get="/partials/status" hx-trigger="every 30s" hx-swap="outerHTML" {
            span class="size-1.5 rounded-full bg-brand-bright shadow-[0_0_0_4px] shadow-brand-bright/10" aria-hidden="true" {}
            "Control plane online"
        }
    }
}
