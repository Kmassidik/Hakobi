/* Live metrics scheduler: 2s poll, 5s backoff after failures, paused while
   the tab is hidden. Replaces fixed-interval HTMX polling on the dashboard. */
"use strict";
{
  const hakobi = (window.Hakobi = window.Hakobi || {});
  const POLL_MS = 2000;
  const BACKOFF_MS = 5000;
  const STALE_AFTER_MS = 30000;
  const CHARTS_MS = 5000;

  async function refreshCharts(state) {
    var response = await fetch(state.chartsUrl, {
      headers: { "HX-Request": "true" },
    });
    if (response.ok) {
      var html = await response.text();
      state.charts.outerHTML = html;
      state.charts = document.querySelector("[data-live-charts]");
      state.chartsAt = Date.now();
    }
  }

  function markFresh(state) {
    var stale = Date.now() - state.lastGoodAt > STALE_AFTER_MS;
    state.grid.querySelectorAll("[data-freshness]").forEach(function (badge) {
      badge.textContent = stale ? "stale" : "live";
      badge.dataset.state = stale ? "stale" : "live";
    });
    if (stale) state.interval = BACKOFF_MS;
  }

  async function tick(state) {
    if (state.busy || document.hidden) return;
    state.busy = true;
    var started = Date.now();
    try {
      var response = await fetch(state.url, { headers: { "HX-Request": "true" } });
      if (response.ok) {
        var html = await response.text();
        state.grid.outerHTML = html;
        state.grid = document.querySelector("[data-live-metrics]");
        state.interval = POLL_MS;
        state.lastGoodAt = Date.now();
        markFresh(state);
        // Refresh charts at a slower cadence than the stat cards.
        if (state.chartsUrl && Date.now() - state.chartsAt >= CHARTS_MS) {
          await refreshCharts(state);
        }
        if (hakobi.drawCharts) hakobi.drawCharts();
      } else {
        state.interval = BACKOFF_MS;
      }
    } catch (error) {
      state.interval = BACKOFF_MS;
    }
    state.busy = false;
    var spent = Date.now() - started;
    setTimeout(function () {
      tick(state);
    }, Math.max(state.interval - spent, 250));
  }

  function init() {
    var grid = document.querySelector("[data-live-metrics]");
    if (!grid) return;
    var charts = document.querySelector("[data-live-charts]");
    var state = {
      grid: grid,
      url: grid.dataset.liveMetrics,
      interval: POLL_MS,
      lastGoodAt: Date.now(),
      busy: false,
      charts: charts,
      chartsUrl: charts ? charts.dataset.liveCharts : null,
      chartsAt: 0,
    };
    document.addEventListener("visibilitychange", function () {
      if (!document.hidden) tick(state);
    });
    tick(state);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
}
