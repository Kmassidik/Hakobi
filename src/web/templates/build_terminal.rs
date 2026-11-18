//! Server-rendered build output with progressive SSE enhancement.

use hakobi::catalog::{Deployment, DeploymentLog};
use maud::{html, Markup};

pub(crate) fn is_active(deployment: &Deployment) -> bool {
    matches!(
        deployment.status.as_str(),
        "pending" | "building" | "deploying"
    )
}

pub(crate) fn status_class(status: &str) -> &'static str {
    match status {
        "running" => "build-terminal__status build-terminal__status--running",
        "pending" | "building" | "deploying" => {
            "build-terminal__status build-terminal__status--active"
        }
        "failed" | "cancelled" => "build-terminal__status build-terminal__status--failed",
        _ => "build-terminal__status build-terminal__status--muted",
    }
}

pub fn build_terminal(
    deployment: &Deployment,
    logs: &[DeploymentLog],
    csrf_token: &str,
    auto_open: bool,
) -> Markup {
    let active = is_active(deployment);
    let started_ms = timestamp_ms(&deployment.started_at).unwrap_or_default();
    let completed_ms = deployment
        .completed_at
        .as_deref()
        .and_then(timestamp_ms)
        .unwrap_or_default();
    let elapsed = initial_elapsed(deployment, started_ms, completed_ms);
    let connection = if active {
        "Connecting"
    } else if auto_open {
        "Replay"
    } else {
        "History"
    };
    html! {
        section class="build-terminal" data-build-terminal data-deployment-id=(deployment.id)
            data-status=(deployment.status) data-started-at-ms=(started_ms)
            data-completed-at-ms=(completed_ms) data-auto-open=(auto_open)
            aria-label=(format!("Deployment v{} build output", deployment.version)) {
            (terminal_header(deployment, csrf_token, active, &elapsed))
            div class="build-terminal__screen" {
                (terminal_toolbar(connection))
                div class="build-terminal__output" data-terminal-output role="log"
                    aria-live="polite" aria-relevant="additions text" tabindex="0" {
                    (terminal_logs(logs))
                }
            }
        }
    }
}

fn terminal_header(deployment: &Deployment, csrf: &str, active: bool, elapsed: &str) -> Markup {
    html! {
        div class="build-terminal__header" {
            div class="build-terminal__summary" {
                h2 class="build-terminal__title" { "v" (deployment.version) }
                span class=(status_class(&deployment.status)) data-terminal-status { (deployment.status) }
                span class="build-terminal__elapsed" data-terminal-elapsed { (elapsed) }
            }
            @if active {
                form method="post" action=(format!("/apps/{}/cancel", deployment.id))
                    class="build-terminal__cancel" data-terminal-cancel {
                    input type="hidden" name="csrf_token" value=(csrf);
                    button type="submit" class="build-terminal__button build-terminal__button--cancel" { "Cancel" }
                }
            }
        }
    }
}

fn terminal_toolbar(connection: &str) -> Markup {
    html! {
        div class="build-terminal__toolbar" {
            span class="build-terminal__connection" data-terminal-connection role="status" {
                span class="build-terminal__connection-dot" aria-hidden="true" {}
                (connection)
            }
            div class="build-terminal__controls" {
                button type="button" class="build-terminal__button" data-terminal-follow
                    aria-pressed="true" title="Keep the newest output in view" { "Follow" }
                button type="button" class="build-terminal__button" data-terminal-resume hidden { "Resume" }
                button type="button" class="build-terminal__button" data-terminal-expand
                    aria-pressed="false" aria-label="Expand build output" title="Expand (Escape to restore)" { "Expand" }
            }
        }
        noscript {
            p class="build-terminal__noscript" { "Live updates require JavaScript. Reload to refresh this output." }
        }
    }
}

fn terminal_logs(logs: &[DeploymentLog]) -> Markup {
    html! {
        @if logs.is_empty() {
            p class="build-terminal__empty" data-terminal-empty { "No log lines recorded." }
        }
        @for log in logs.iter().rev().take(200).collect::<Vec<_>>().into_iter().rev() {
            div class="build-terminal__line" data-log-id=(log.id) {
                span class="build-terminal__timestamp" { (log.timestamp) " " }
                span class=(level_class(&log.level)) { (log.level) }
                " " (log.message)
            }
        }
    }
}

fn level_class(level: &str) -> &'static str {
    match level {
        "error" => "build-terminal__level build-terminal__level--error",
        "warn" => "build-terminal__level build-terminal__level--warn",
        _ => "build-terminal__level build-terminal__level--info",
    }
}

fn initial_elapsed(deployment: &Deployment, started: i64, completed: i64) -> String {
    if completed > 0 && started > 0 {
        return format_duration((completed - started).max(0));
    }
    if is_active(deployment) && started > 0 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis() as i64)
            .unwrap_or(started);
        return format_duration((now - started).max(0));
    }
    "00:00".into()
}

pub(crate) fn deployment_duration(deployment: &Deployment) -> String {
    let started = timestamp_ms(&deployment.started_at).unwrap_or_default();
    let completed = deployment
        .completed_at
        .as_deref()
        .and_then(timestamp_ms)
        .unwrap_or_default();
    if started == 0 || completed == 0 {
        "—".into()
    } else {
        format_duration(completed - started)
    }
}

fn format_duration(milliseconds: i64) -> String {
    let seconds = milliseconds.max(0) / 1000;
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            (seconds / 60) % 60,
            seconds % 60
        )
    } else {
        format!("{:02}:{:02}", seconds / 60, seconds % 60)
    }
}

fn timestamp_ms(value: &str) -> Option<i64> {
    let year = value.get(0..4)?.parse::<i64>().ok()?;
    let month = value.get(5..7)?.parse::<i64>().ok()?;
    let day = value.get(8..10)?.parse::<i64>().ok()?;
    let hour = value.get(11..13)?.parse::<i64>().ok()?;
    let minute = value.get(14..16)?.parse::<i64>().ok()?;
    let second = value.get(17..19)?.parse::<i64>().ok()?;
    let days = days_from_civil(year, month, day);
    Some((((days * 24 + hour) * 60 + minute) * 60 + second) * 1000)
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let adjusted_year = year - i64::from(month <= 2);
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let shifted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146097 + day_of_era - 719468
}
