use maud::{html, Markup};

use super::{AppFormValues, CONTROL};

pub(super) fn runtime_fields(values: &AppFormValues) -> Markup {
    html! {
        (process_fields(values))
        (data_fields(values))
    }
}

pub(super) fn process_fields(values: &AppFormValues) -> Markup {
    html! {
        section class="grid gap-5 border-t border-zinc-200 pt-5" aria-labelledby="processes-title" {
            h3 id="processes-title" class="m-0 text-[13px] font-semibold text-zinc-800" { "Processes" }
            (command_field("start_command", "Web start command", &values.start_command, "npm start", "Optional. Runs through /bin/sh -c and replaces the image CMD; the image ENTRYPOINT is preserved."))
            (command_field("release_command", "Release command", &values.release_command, "npm run migrate", "Optional. Runs once after the build and before production containers are replaced. A non-zero exit fails the deployment."))
            (process_editor(values))
        }
    }
}

pub(super) fn data_fields(values: &AppFormValues) -> Markup {
    html! {
        (volume_editor(values))
        (secret_editor(values))
    }
}

fn command_field(name: &str, label: &str, value: &str, placeholder: &str, help: &str) -> Markup {
    html! {
        label class="grid gap-2 text-[13px] font-semibold text-zinc-800" {
            (label)
            input class=(CONTROL) name=(name) type="text" value=(value) maxlength="4096" placeholder=(placeholder) autocomplete="off" spellcheck="false";
            span class="text-[11.5px] font-normal leading-relaxed text-zinc-500" { (help) }
        }
    }
}

fn process_editor(values: &AppFormValues) -> Markup {
    html! {
        (editor_heading("Worker processes", "Private containers built from the same image. They share variables, secrets, volumes, and resource limits, but publish no ports.", "processes-add", "Add worker"))
        (json_fallback("processes_json", &values.processes_json, "Worker processes (JSON)"))
        div data-runtime-editor="processes" hidden class="grid gap-3" {
            (row_headers(&["Name", "Command"]))
            div data-runtime-rows class="grid gap-3" {}
            template data-runtime-template {
                div data-runtime-row class="grid grid-cols-1 gap-3 rounded-lg border border-zinc-200 p-3 sm:grid-cols-[1fr_2fr_auto] sm:items-end" {
                    (text_input("name", "Name", "worker"))
                    (text_input("command", "Command", "npm run worker"))
                    (remove_button("Remove worker"))
                }
            }
            p data-runtime-error role="alert" class="m-0 text-[11.5px] text-danger" {}
        }
    }
}

fn volume_editor(values: &AppFormValues) -> Markup {
    html! {
        section class="grid gap-4 border-t border-zinc-200 pt-5" aria-labelledby="volumes-title" {
            h3 id="volumes-title" class="sr-only" { "Persistent volumes" }
            (editor_heading("Persistent volumes", "Named volumes survive rebuilds and rollbacks. Removing a mount or app does not delete data unless you explicitly confirm data deletion.", "volumes-add", "Add volume"))
            (json_fallback("volumes_json", &values.volumes_json, "Persistent volumes (JSON)"))
            div data-runtime-editor="volumes" hidden class="grid gap-3" {
                (row_headers(&["Volume key", "Mount path", "Access"]))
                div data-runtime-rows class="grid gap-3" {}
                template data-runtime-template {
                    div data-runtime-row class="grid grid-cols-1 gap-3 rounded-lg border border-zinc-200 p-3 sm:grid-cols-[1fr_2fr_auto_auto] sm:items-end" {
                        (text_input("name", "Volume key", "data"))
                        (text_input("mount_path", "Mount path", "/var/lib/app"))
                        label class="grid min-h-10 gap-1.5 text-[11px] font-semibold text-zinc-500" {
                            "Access"
                            select data-field="read_only" class=(CONTROL) {
                                option value="false" { "Read/write" }
                                option value="true" { "Read-only" }
                            }
                        }
                        (remove_button("Remove volume"))
                    }
                }
                p data-runtime-error role="alert" class="m-0 text-[11.5px] text-danger" {}
            }
        }
    }
}

fn secret_editor(values: &AppFormValues) -> Markup {
    html! {
        section class="grid gap-4 border-t border-zinc-200 pt-5" aria-labelledby="secrets-title" {
            h3 id="secrets-title" class="sr-only" { "Encrypted secrets" }
            (editor_heading("Encrypted secrets", "Use for DATABASE_URL, REDIS_URL, R2 credentials, and other sensitive runtime values. Values are write-only and blank values keep existing secrets.", "secrets-add", "Add secret"))
            (json_fallback("secrets_json", &values.secrets_json, "Encrypted secrets (JSON)"))
            div data-runtime-editor="secrets" hidden class="grid gap-3" {
                (row_headers(&["Name", "Value"]))
                div data-runtime-rows class="grid gap-3" {}
                template data-runtime-template {
                    div data-runtime-row class="grid grid-cols-1 gap-3 rounded-lg border border-zinc-200 p-3 sm:grid-cols-[1fr_2fr_auto] sm:items-end" {
                        (text_input("name", "Name", "DATABASE_URL"))
                        label class="grid gap-1.5 text-[11px] font-semibold text-zinc-500" {
                            "Value"
                            input data-field="value" data-secret-value class=(CONTROL) type="password" placeholder="Value or leave unchanged" autocomplete="new-password";
                        }
                        (remove_button("Remove secret"))
                    }
                }
                p data-runtime-error role="alert" class="m-0 text-[11.5px] text-danger" {}
            }
        }
    }
}

fn editor_heading(title: &str, description: &str, add: &str, label: &str) -> Markup {
    html! {
        div class="flex items-start justify-between gap-4" {
            div { h4 class="m-0 text-[13px] font-semibold text-zinc-800" { (title) }
                p class="mb-0 mt-1 text-[11.5px] leading-relaxed text-zinc-500" { (description) }
            }
            button type="button" data-runtime-add=(add) class="inline-flex min-h-9 shrink-0 items-center rounded-lg border border-zinc-200 bg-white px-3 text-[12px] font-semibold text-zinc-700 hover:bg-zinc-50" { (label) }
        }
    }
}

fn json_fallback(name: &str, value: &str, label: &str) -> Markup {
    html! { label data-runtime-raw class="grid gap-2 text-[13px] font-semibold text-zinc-800" {
        (label)
        textarea class=(CONTROL) name=(name) data-runtime-json rows="3" autocomplete="off" { (value) }
    } }
}

fn row_headers(labels: &[&str]) -> Markup {
    html! { div class="hidden text-[11px] font-semibold text-zinc-500 sm:flex sm:gap-3" {
        @for label in labels { span class="flex-1" { (label) } }
    } }
}

fn text_input(field: &str, label: &str, placeholder: &str) -> Markup {
    html! { label class="grid gap-1.5 text-[11px] font-semibold text-zinc-500" {
        (label)
        input data-field=(field) class=(CONTROL) type="text" placeholder=(placeholder) autocomplete="off" spellcheck="false";
    } }
}

fn remove_button(label: &str) -> Markup {
    html! { button type="button" data-runtime-remove aria-label=(label) class="inline-flex min-h-10 items-center justify-center rounded-lg border border-danger/30 bg-white px-3 text-danger hover:bg-danger/5" {
        i class="fa-solid fa-trash-can" aria-hidden="true" {}
    } }
}
