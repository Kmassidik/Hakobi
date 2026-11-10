//! Shared layout for Maud SSR templates: page shell, sidebar, and badges.

mod container_detail;
mod containers;
mod dashboard;
pub(crate) mod deployment;
mod environment_form;
mod environments;
mod history;
mod hosts;
mod infrastructure;
mod providers;
mod readiness;
mod stacks;

pub(crate) use container_detail::{
    container_detail, container_logs_error, container_logs_section, container_stats_error,
    container_stats_section, ContainerDetailData,
};
pub use containers::{container_form, containers, ContainerFormValues};
pub use dashboard::{dashboard, metrics_section, ActivityRow, DashboardMetrics, DashboardView};
pub use deployment::{deployment_page, deployment_rows, environment_webhook};
pub use environment_form::{environment_form, EnvironmentFormValues};
pub use environments::{
    environment_deployments, environment_detail, environment_logs, environment_status_body,
    environments,
};
pub use history::{history_panel, history_window_bounds, HistoryWindow};
pub use hosts::{host_resources, host_resources_live, hosts};
pub use infrastructure::infrastructure;
pub use providers::providers;
pub use readiness::readiness_section;
pub use stacks::{stack_delete, stack_detail, stack_form, stacks, StackFormValues};

use maud::{html, Markup, DOCTYPE};

pub(crate) const PAGE_BODY: &str = "min-w-80 bg-paper font-sans text-ink antialiased";
pub(crate) const BRAND: &str =
    "inline-flex items-center gap-3 text-[1.12rem] font-bold tracking-[-0.03em] no-underline";
pub(crate) const BRAND_MARK: &str =
    "grid size-9 place-items-center rounded-[0.65rem] bg-lime font-extrabold text-brand";
pub(crate) const CONTENT_WIDTH: &str = "mx-auto w-[90%] max-w-[78rem]";
pub(crate) const EYEBROW: &str = "eyebrow";
pub(crate) const FIELD: &str = "w-full rounded-[0.6rem] border border-line bg-surface px-4 py-3.5 text-ink outline-none transition focus:border-brand-bright focus:ring-[3px] focus:ring-brand-bright/15";

pub(crate) fn page(title: &str, content: Markup) -> Markup {
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

pub(crate) fn brand(home_label: &str) -> Markup {
    html! {
        a class=(BRAND) href="/" aria-label=(home_label) {
            span class=(BRAND_MARK) aria-hidden="true" { "運" }
            span { "Hakobi" }
        }
    }
}

fn nav_item(active: &str, key: &str, href: &str, icon: &str, label: &str) -> Markup {
    let class = if active == key {
        "relative flex items-center gap-3 rounded-lg bg-slate-100 px-3.5 py-2.5 text-sm font-semibold text-slate-900 no-underline before:absolute before:left-0 before:top-1/2 before:h-5 before:w-[3px] before:-translate-y-1/2 before:rounded-full before:bg-brand"
    } else {
        "flex items-center gap-3 rounded-lg px-3.5 py-2.5 text-sm font-semibold text-slate-500 no-underline transition hover:bg-slate-50 hover:text-slate-900"
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
        "flex min-w-0 flex-col items-center gap-1 rounded-lg bg-slate-100 px-1 py-2 text-[0.62rem] font-bold text-slate-900 no-underline"
    } else {
        "flex min-w-0 flex-col items-center gap-1 rounded-lg px-1 py-2 text-[0.62rem] font-semibold text-slate-500 no-underline transition hover:bg-slate-50 hover:text-slate-900"
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
        aside class="fixed inset-y-0 left-0 z-40 hidden w-64 flex-col bg-white px-5 py-6 lg:flex" aria-label="Sidebar" {
            div class="px-1" { (brand("Hakobi dashboard")) }
            nav class="mt-9 flex flex-col gap-1" aria-label="Primary" {
                (nav_item(active, "overview", "/", "fa-square-poll-vertical", "Overview"))
                (nav_item(active, "environments", "/environments", "fa-layer-group", "Environments"))
                (nav_item(active, "stacks", "/stacks", "fa-cubes-stacked", "Stacks"))
                (nav_item(active, "containers", "/containers", "fa-box", "Containers"))
                (nav_item(active, "infrastructure", "/infrastructure", "fa-server", "Infrastructure"))
                (nav_item(active, "providers", "/providers", "fa-plug", "Providers"))
                (nav_item(active, "deployment", "/hosts", "fa-rocket", "Deployment"))
            }
            div class="mt-auto" {
                div class="rounded-xl bg-[#f5f5f7] p-3 shadow-[0_1px_3px_rgba(0,0,0,0.04)]" {
                    div class="flex items-center gap-3" {
                        span class="grid size-9 shrink-0 place-items-center rounded-full bg-brand text-xs font-bold uppercase text-white" aria-hidden="true" {
                            (username.chars().next().unwrap_or('H'))
                        }
                        div class="min-w-0" {
                            p class="m-0 truncate text-sm font-semibold text-slate-900" { (username) }
                            p class="m-0 truncate text-[0.68rem] text-slate-500" { (base_url.trim_start_matches("https://").trim_start_matches("http://")) }
                        }
                    }
                    div class="mt-3 flex items-center justify-between gap-2" {
                        span class="inline-flex items-center gap-1.5 text-[0.68rem] font-medium text-slate-500" {
                            span class="size-1.5 animate-pulse rounded-full bg-emerald-500" aria-hidden="true" {}
                            "Online"
                        }
                        form method="post" action="/logout" class="m-0" {
                            input type="hidden" name="csrf_token" value=(csrf_token);
                            button class="inline-flex items-center gap-1.5 rounded-lg px-2 py-1.5 text-[0.7rem] font-semibold text-slate-600 transition hover:bg-white hover:text-slate-900" type="submit" {
                                i class="fa-solid fa-right-from-bracket" aria-hidden="true" {}
                                "Sign out"
                            }
                        }
                    }
                }
            }
        }
        (mobile_sidebar(active, base_url, username, csrf_token))
    }
}

pub(crate) fn login(
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

pub(crate) fn status_badge(label: &str, healthy: bool) -> Markup {
    let dot = if healthy {
        "size-2 animate-pulse rounded-full bg-emerald-500 shadow-[0_0_0_4px] shadow-emerald-500/10"
    } else {
        "size-2 rounded-full bg-amber-400 shadow-[0_0_0_4px] shadow-amber-400/10"
    };
    html! {
        span id="control-status" class="inline-flex items-center gap-2 whitespace-nowrap text-xs font-medium text-[#6e6e73]" {
            span class=(dot) aria-hidden="true" {}
            (label)
        }
    }
}

pub(crate) fn format_bytes(bytes: u64) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    if bytes as f64 >= GIB {
        format!("{:.1} GB", bytes as f64 / GIB)
    } else {
        format!("{:.0} MB", bytes as f64 / MIB)
    }
}

fn mobile_sidebar(active: &str, _base_url: &str, _username: &str, _csrf_token: &str) -> Markup {
    html! {
            nav class="fixed inset-x-0 bottom-0 z-40 bg-white/95 px-2 pb-2 pt-1.5 shadow-[0_-1px_3px_rgba(0,0,0,0.04)] backdrop-blur lg:hidden" aria-label="Mobile primary" {
                div class="mx-auto grid max-w-md grid-cols-6 gap-0.5" {
                    (mobile_nav_item(active, "overview", "/", "fa-square-poll-vertical", "Overview"))
                    (mobile_nav_item(active, "environments", "/environments", "fa-layer-group", "Envs"))
                    (mobile_nav_item(active, "stacks", "/stacks", "fa-cubes-stacked", "Stacks"))
                    (mobile_nav_item(active, "containers", "/containers", "fa-box", "Containers"))
                    (mobile_nav_item(active, "infrastructure", "/infrastructure", "fa-server", "Infra"))
                    (mobile_nav_item(active, "providers", "/providers", "fa-plug", "Providers"))
                    (mobile_nav_item(active, "deployment", "/hosts", "fa-rocket", "Deploy"))
                }
            }
    }
}
