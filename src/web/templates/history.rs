//! Server-rendered SVG history charts for the host resources page (CP7).
//!
//! Charts are plain `<svg>` markup using CSS variables for colors; no chart
//! library and no inline styles.

use maud::{html, Markup, PreEscaped};

use hakobi::metrics::HistoryPoint;

/// History windows offered by the selector, with their rollup source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryWindow {
    /// Last 5 minutes, from 1-minute rollups.
    FiveMinutes,
    /// Last hour, from 5-minute rollups.
    OneHour,
    /// Last 24 hours, from 1-hour rollups.
    TwentyFourHours,
}

impl HistoryWindow {
    /// Parses the `?window=` query value; unknown values fall back to 1h.
    pub fn from_query(value: Option<&str>) -> Self {
        match value {
            Some("5m") => Self::FiveMinutes,
            Some("24h") => Self::TwentyFourHours,
            _ => Self::OneHour,
        }
    }

    /// Query parameter value for this window.
    pub fn query_value(self) -> &'static str {
        match self {
            Self::FiveMinutes => "5m",
            Self::OneHour => "1h",
            Self::TwentyFourHours => "24h",
        }
    }

    /// Window length in milliseconds.
    fn span_ms(self) -> i64 {
        match self {
            Self::FiveMinutes => 5 * 60_000,
            Self::OneHour => 60 * 60_000,
            Self::TwentyFourHours => 24 * 60 * 60_000,
        }
    }

    /// The other windows, for the selector.
    fn others(self) -> [Self; 2] {
        match self {
            Self::FiveMinutes => [Self::OneHour, Self::TwentyFourHours],
            Self::OneHour => [Self::FiveMinutes, Self::TwentyFourHours],
            Self::TwentyFourHours => [Self::FiveMinutes, Self::OneHour],
        }
    }
}

/// Time axis bounds: window start and end in milliseconds.
pub(crate) fn time_bounds(window: HistoryWindow, now_ms: i64) -> (i64, i64) {
    (now_ms - window.span_ms(), now_ms)
}

/// Public window bounds for handlers computing query ranges.
pub fn history_window_bounds(window: HistoryWindow, now_ms: i64) -> (i64, i64) {
    time_bounds(window, now_ms)
}

/// SVG viewBox geometry shared by every chart.
const WIDTH: f64 = 600.0;
const HEIGHT: f64 = 160.0;
const PADDING: f64 = 6.0;

/// Maps points to SVG polyline coordinates inside the viewBox.
fn polyline(points: &[(f64, f64)]) -> String {
    points
        .iter()
        .map(|(x, y)| format!("{x:.1},{y:.1}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Renders a percentage line chart (CPU, memory). The y-axis is always 0–100%.
pub(crate) fn percent_chart(points: &[(i64, f64)], from: i64, to: i64, label: &str) -> Markup {
    let span = (to - from).max(1) as f64;
    let coords: Vec<(f64, f64)> = points
        .iter()
        .map(|(timestamp, value)| {
            let x = PADDING + (WIDTH - 2.0 * PADDING) * (*timestamp - from) as f64 / span;
            let y = HEIGHT - PADDING - (HEIGHT - 2.0 * PADDING) * value.clamp(0.0, 100.0) / 100.0;
            (x, y)
        })
        .collect();
    let latest = points.last().map(|(_, value)| *value).unwrap_or(0.0);
    html! {
        figure class="m-0" aria-label=(format!("{label} history chart")) {
            svg viewBox=(format!("0 0 {WIDTH} {HEIGHT}")) class="chart-svg" role="img" preserveAspectRatio="none" {
                line x1=(PADDING) x2=(WIDTH - PADDING) y1=(HEIGHT / 2.0) y2=(HEIGHT / 2.0) class="chart-grid" {}
                @if coords.len() >= 2 {
                    polyline points=(PreEscaped(polyline(&coords))) class="chart-line" {}
                } @else {
                    text x=(WIDTH / 2.0) y=(HEIGHT / 2.0) class="chart-empty" text-anchor="middle" { "No data yet" }
                }
            }
            figcaption class="mt-1 text-[0.68rem] text-[#86868b]" { (label) " · now " (format!("{latest:.1}%")) }
        }
    }
}

/// Renders a throughput line chart (network RX/TX) auto-scaled to the max.
pub(crate) fn throughput_chart(
    points: &[(i64, f64)],
    tx: &[(i64, f64)],
    from: i64,
    to: i64,
    label: &str,
) -> Markup {
    let span = (to - from).max(1) as f64;
    let max = points
        .iter()
        .chain(tx.iter())
        .map(|(_, value)| *value)
        .fold(1.0_f64, f64::max);
    let to_coords = |series: &[(i64, f64)]| -> Vec<(f64, f64)> {
        series
            .iter()
            .map(|(timestamp, value)| {
                let x = PADDING + (WIDTH - 2.0 * PADDING) * (*timestamp - from) as f64 / span;
                let y = HEIGHT - PADDING - (HEIGHT - 2.0 * PADDING) * (value / max).clamp(0.0, 1.0);
                (x, y)
            })
            .collect()
    };
    let rx_coords = to_coords(points);
    let tx_coords = to_coords(tx);
    let latest_rx = points.last().map(|(_, value)| *value).unwrap_or(0.0);
    let latest_tx = tx.last().map(|(_, value)| *value).unwrap_or(0.0);
    html! {
        figure class="m-0" aria-label=(format!("{label} history chart")) {
            svg viewBox=(format!("0 0 {WIDTH} {HEIGHT}")) class="chart-svg" role="img" preserveAspectRatio="none" {
                line x1=(PADDING) x2=(WIDTH - PADDING) y1=(HEIGHT / 2.0) y2=(HEIGHT / 2.0) class="chart-grid" {}
                @if rx_coords.len() >= 2 {
                    polyline points=(PreEscaped(polyline(&rx_coords))) class="chart-line" {}
                }
                @if tx_coords.len() >= 2 {
                    polyline points=(PreEscaped(polyline(&tx_coords))) class="chart-line chart-line--tx" {}
                }
                @if rx_coords.len() < 2 && tx_coords.len() < 2 {
                    text x=(WIDTH / 2.0) y=(HEIGHT / 2.0) class="chart-empty" text-anchor="middle" { "No data yet" }
                }
            }
            figcaption class="mt-1 text-[0.68rem] text-[#86868b]" {
                (label) " · RX " (format!("{latest_rx:.0}")) " B/s · TX " (format!("{latest_tx:.0}")) " B/s · peak " (format!("{max:.0}"))
            }
        }
    }
}

/// Renders a disk usage bar chart: one bar per sampled disk-used value.
pub(crate) fn disk_chart(points: &[(i64, u64)], from: i64, to: i64, label: &str) -> Markup {
    let span = (to - from).max(1) as f64;
    let max = points.iter().map(|(_, value)| *value).fold(1_u64, u64::max) as f64;
    let bar_width = ((WIDTH - 2.0 * PADDING) / points.len().max(1) as f64 * 0.7).min(24.0);
    let latest = points.last().map(|(_, value)| *value).unwrap_or(0);
    html! {
        figure class="m-0" aria-label=(format!("{label} history chart")) {
            svg viewBox=(format!("0 0 {WIDTH} {HEIGHT}")) class="chart-svg" role="img" preserveAspectRatio="none" {
                @if points.is_empty() {
                    text x=(WIDTH / 2.0) y=(HEIGHT / 2.0) class="chart-empty" text-anchor="middle" { "No data yet" }
                } @else {
                    @for (timestamp, used) in points {
                        @let x = PADDING + (WIDTH - 2.0 * PADDING) * (*timestamp - from) as f64 / span - bar_width / 2.0;
                        @let bar_height = (HEIGHT - 2.0 * PADDING) * (*used as f64 / max).clamp(0.0, 1.0);
                        rect x=(x.max(PADDING)) y=(HEIGHT - PADDING - bar_height) width=(bar_width) height=(bar_height) class="chart-bar" rx="1" {}
                    }
                }
            }
            figcaption class="mt-1 text-[0.68rem] text-[#86868b]" { (label) " · now " (super::format_bytes(latest)) }
        }
    }
}

/// The time-window selector: 5m / 1h / 24h links reloading the history partial.
pub fn window_selector(host_id: &str, active: HistoryWindow) -> Markup {
    html! {
        div class="flex items-center gap-2" role="tablist" aria-label="History time window" {
            @for window in active.others() {
                a class="rounded-full bg-white px-3 py-1.5 text-[0.72rem] font-semibold text-ink no-underline shadow-sm"
                  href=(format!("/hosts/{host_id}/resources/history?window={}", window.query_value())) {
                    (window.query_value())
                }
            }
            span class="rounded-full bg-brand px-3 py-1.5 text-[0.72rem] font-semibold text-white" {
                (active.query_value())
            }
        }
    }
}

/// The full history panel: selector plus the four charts.
pub fn history_panel(
    host_id: &str,
    window: HistoryWindow,
    points: &[HistoryPoint],
    now_ms: i64,
) -> Markup {
    let (from, to) = time_bounds(window, now_ms);
    let cpu: Vec<(i64, f64)> = points
        .iter()
        .map(|p| (p.timestamp, p.cpu_percent))
        .collect();
    let memory: Vec<(i64, f64)> = points
        .iter()
        .map(|p| (p.timestamp, p.memory_percent))
        .collect();
    let rx: Vec<(i64, f64)> = points
        .iter()
        .map(|p| (p.timestamp, p.network_rx_bps))
        .collect();
    let tx: Vec<(i64, f64)> = points
        .iter()
        .map(|p| (p.timestamp, p.network_tx_bps))
        .collect();
    let disk: Vec<(i64, u64)> = points
        .iter()
        .map(|p| (p.timestamp, p.disk_used_bytes))
        .collect();
    html! {
        div id="host-history" {
            div class="mb-4 flex items-center justify-between gap-4" {
                h2 class="m-0 text-[14px] font-semibold" { "History" }
                (window_selector(host_id, window))
            }
            div class="grid grid-cols-1 gap-4 xl:grid-cols-2" {
                section class="rounded-xl bg-white p-5 shadow-[0_1px_3px_rgba(0,0,0,0.04)]" aria-label="CPU history" {
                    (percent_chart(&cpu, from, to, "CPU"))
                }
                section class="rounded-xl bg-white p-5 shadow-[0_1px_3px_rgba(0,0,0,0.04)]" aria-label="Memory history" {
                    (percent_chart(&memory, from, to, "Memory"))
                }
                section class="rounded-xl bg-white p-5 shadow-[0_1px_3px_rgba(0,0,0,0.04)]" aria-label="Disk history" {
                    (disk_chart(&disk, from, to, "Disk used"))
                }
                section class="rounded-xl bg-white p-5 shadow-[0_1px_3px_rgba(0,0,0,0.04)]" aria-label="Network history" {
                    (throughput_chart(&rx, &tx, from, to, "Network"))
                }
            }
        }
    }
}
