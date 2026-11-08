//! Host readiness section and readiness cards.

use maud::{html, Markup};

use hakobi::install::{Readiness, ToolStatus};

pub fn readiness_section(
    readiness: &Readiness,
    csrf_token: &str,
    agent_online: bool,
    agent_configured: bool,
) -> Markup {
    let note = if agent_online {
        "Detected by the host agent. Installs run on the host."
    } else if agent_configured {
        "Host agent unreachable. Host installs are disabled."
    } else {
        "Detected locally. Nothing is installed automatically."
    };
    html! {
        section id="overview-readiness" aria-labelledby="readiness-title" hx-get="/partials/readiness" hx-trigger="every 30s" hx-swap="outerHTML" {
            div class="mb-3 flex flex-col gap-1 sm:flex-row sm:items-end sm:justify-between" {
                h2 id="readiness-title" class="m-0 text-[1.05rem] font-semibold tracking-[-0.02em]" { "Host readiness" }
                p class="m-0 text-xs text-[#86868b]" { (note) }
            }
            div class="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-5" {
                (readiness_card("Docker", "/assets/art/3d/deploy.webp", "docker", &readiness.docker, csrf_token, agent_online, agent_configured))
                (readiness_card("Compose", "/assets/art/3d/stack-compose.webp", "compose", &readiness.compose, csrf_token, agent_online, agent_configured))
                (readiness_card("kubectl", "/assets/art/3d/domain.webp", "kubectl", &readiness.kubectl, csrf_token, agent_online, agent_configured))
                (readiness_card("Nix", "/assets/art/3d/source-repo.webp", "nix", &readiness.nix, csrf_token, agent_online, agent_configured))
                (readiness_card("Nixpacks", "/assets/art/3d/build-runner.webp", "nixpacks", &readiness.nixpacks, csrf_token, agent_online, agent_configured))
            }
        }
    }
}

fn readiness_card(
    label: &str,
    art: &str,
    tool: &str,
    status: &ToolStatus,
    csrf_token: &str,
    can_install: bool,
    agent_configured: bool,
) -> Markup {
    html! {
        article class="rounded-xl bg-white p-4 shadow-[0_1px_3px_rgba(0,0,0,0.04)]" {
            div class="flex items-center gap-3" {
                span class="grid size-9 place-items-center rounded-lg bg-brand/10" aria-hidden="true" {
                    img src=(art) alt="" class="size-7 object-contain" loading="lazy";
                }
                h3 class="m-0 text-[0.95rem] font-semibold" { (label) }
            }
            @match status {
                ToolStatus::Ready { version } => {
                    p class="mb-0 mt-3 flex items-center gap-2 text-xs text-slate-500" {
                        span class="size-1.5 rounded-full bg-emerald-500" aria-hidden="true" {}
                        "Ready · "
                        code class="min-w-0 truncate font-mono text-slate-700" title=(version) { (version) }
                    }
                },
                ToolStatus::Missing { install_command } => {
                    p class="mb-0 mt-3 flex items-center gap-2 text-xs text-slate-500" {
                        span class="size-1.5 rounded-full bg-amber-400" aria-hidden="true" {}
                        "Missing"
                    }
                    @if can_install {
                        form method="post" action=(format!("/host/install/{tool}")) class="mt-3" {
                            input type="hidden" name="csrf_token" value=(csrf_token);
                            button class="rounded-lg bg-brand px-3 py-1.5 text-[0.72rem] font-semibold text-white transition hover:bg-[#0f4130]" type="submit" {
                                "Install on host"
                            }
                        }
                    } @else if agent_configured {
                        p class="mb-0 mt-3 text-[0.72rem] font-semibold text-amber-700" { "Agent offline" }
                    } @else {
                        button type="button" class="mt-3 rounded-lg bg-slate-100 px-3 py-1.5 text-[0.72rem] font-semibold text-slate-700 transition hover:bg-slate-200" data-dialog-open="install-tool" data-install-title=(label) data-install-command=(install_command) {
                            "Show install command"
                        }
                    }
                },
            }
        }
    }
}
