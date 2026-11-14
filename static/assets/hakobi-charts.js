/* Dashboard time-series charts: draws canvas[data-chart] elements using the
   series JSON in data-series. Percent charts are fixed 0-100; auto charts
   scale to the data. */
"use strict";
{
  const hakobi = (window.Hakobi = window.Hakobi || {});
  const COLORS = ["#164f3b", "#d97706", "#3b82f6", "#8b5cf6", "#0891b2"];

  function chartMaximum(series, isPercent) {
    var max = 100;
    if (isPercent) {
      var peak = 0;
      series.forEach(function (s) {
        s.values.forEach(function (v) {
          if (v > peak) peak = v;
        });
      });
      if (peak < 10) max = niceCeil(Math.max(peak * 1.3, 2));
    } else {
      max = 1;
      series.forEach(function (s) {
        s.values.forEach(function (v) {
          if (v > max) max = v;
        });
      });
      max = niceCeil(max * 1.15);
    }
    return max;
  }

  function drawAxes(ctx, width, height, pad, cw, ch, max, isPercent, count) {
    ctx.font = "10px ui-monospace, monospace";
    ctx.fillStyle = "#a1a1a6";
    ctx.strokeStyle = "#ececf0";
    ctx.lineWidth = 1;
    var i, y;
    for (i = 0; i <= 4; i++) {
      y = pad.top + (i * ch) / 4;
      ctx.beginPath();
      ctx.moveTo(pad.left, y);
      ctx.lineTo(width - pad.right, y);
      ctx.stroke();
      ctx.fillText(formatAxis(max * (1 - i / 4), isPercent), 4, y + 3);
    }
    if (count >= 2) {
      ["-60m", "-45m", "-30m", "-15m", "now"].forEach(function (label, idx) {
        ctx.fillText(label, pad.left + (idx * cw) / 4 - 10, height - 5);
      });
    }
  }

  function drawSeries(ctx, series, pad, cw, ch, count, max) {
    series.forEach(function (s, si) {
      ctx.beginPath();
      s.values.forEach(function (v, vi) {
        var x = pad.left + (vi * cw) / Math.max(1, count - 1);
        var yy = pad.top + ch * (1 - Math.max(0, Math.min(max, v)) / max);
        if (vi) ctx.lineTo(x, yy);
        else ctx.moveTo(x, yy);
      });
      ctx.strokeStyle = s.color || COLORS[si % COLORS.length];
      ctx.lineWidth = 1.8;
      ctx.lineJoin = "round";
      ctx.stroke();
    });
  }

  function draw(canvas) {
    var series;
    try {
      series = JSON.parse(canvas.dataset.series || "[]");
    } catch (error) {
      return;
    }
    if (!series.length) {
      drawEmpty(canvas);
      return;
    }
    var dpr = window.devicePixelRatio || 1;
    var width = Math.max(250, canvas.clientWidth);
    var height = canvas.clientHeight || 150;
    canvas.width = width * dpr;
    canvas.height = height * dpr;
    var ctx = canvas.getContext("2d");
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, width, height);
    var pad = { left: 36, right: 10, top: 10, bottom: 20 };
    var cw = width - pad.left - pad.right;
    var ch = height - pad.top - pad.bottom;
    var isPercent = canvas.dataset.chart === "percent";
    var max = chartMaximum(series, isPercent);
    var count = series[0].values.length;
    drawAxes(ctx, width, height, pad, cw, ch, max, isPercent, count);
    drawSeries(ctx, series, pad, cw, ch, count, max);
  }

  function drawEmpty(canvas) {
    var ctx = canvas.getContext("2d");
    var dpr = window.devicePixelRatio || 1;
    var width = Math.max(250, canvas.clientWidth);
    var height = canvas.clientHeight || 150;
    canvas.width = width * dpr;
    canvas.height = height * dpr;
    ctx.scale(dpr, dpr);
    ctx.font = "12px -apple-system, sans-serif";
    ctx.fillStyle = "#86868b";
    ctx.textAlign = "center";
    ctx.fillText("No data yet", width / 2, height / 2);
    ctx.textAlign = "left";
  }

  function niceCeil(value) {
    if (value <= 0) return 1;
    var pow = Math.pow(10, Math.floor(Math.log10(value)));
    var unit = value / pow;
    var nice = unit <= 1 ? 1 : unit <= 2 ? 2 : unit <= 5 ? 5 : 10;
    return nice * pow;
  }

  function formatAxis(value, isPercent) {
    if (isPercent) return Math.round(value) + "%";
    if (value >= 1024) return (value / 1024).toFixed(1) + "k";
    if (value >= 10) return Math.round(value).toString();
    return value.toFixed(1);
  }

  function drawAll() {
    document.querySelectorAll("canvas[data-chart]").forEach(draw);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", drawAll);
  } else {
    drawAll();
  }
  window.addEventListener("resize", drawAll);
  hakobi.drawCharts = drawAll;
  document.body.addEventListener("htmx:afterSwap", drawAll);
}
