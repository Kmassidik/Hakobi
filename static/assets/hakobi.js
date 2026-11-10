(() => {
  "use strict";

  const openDialogs = (button) => {
    const id = button.getAttribute("data-dialog-open");
    if (!id) {
      return;
    }
    const dialog = document.getElementById(id);
    if (!dialog) {
      return;
    }

    if (id === "install-tool") {
      const title = dialog.querySelector("[data-install-title]");
      const command = dialog.querySelector("[data-install-command]");
      const copy = dialog.querySelector("[data-copy-command]");
      const label = button.getAttribute("data-install-title") || "this tool";
      const text = button.getAttribute("data-install-command") || "";
      if (title) {
        title.textContent = "Install " + label;
      }
      if (command) {
        command.textContent = text;
      }
      if (copy) {
        copy.textContent = "Copy command";
        copy.removeAttribute("data-copied");
      }
    }

    if (id === "delete-stack" || id === "delete-container") {
      const form = dialog.querySelector("form");
      const name = dialog.querySelector("[data-confirm-name]");
      const input = dialog.querySelector("[data-confirm-input]");
      const submit = dialog.querySelector("[data-confirm-submit]");
      const expected = button.getAttribute("data-delete-name") || "";
      if (form) {
        form.setAttribute(
          "action",
          button.getAttribute("data-delete-action") || "",
        );
      }
      if (name) {
        name.textContent = expected;
      }
      if (input) {
        input.value = "";
        input.setAttribute("aria-invalid", "false");
      }
      if (submit) {
        submit.disabled = true;
      }
      dialog.setAttribute("data-expected-name", expected);
    }

    dialog.showModal();
  };

  document.addEventListener("click", (event) => {
    const target = event.target instanceof Element ? event.target : null;
    if (!target) {
      return;
    }

    const opener = target.closest("[data-dialog-open]");
    if (opener) {
      openDialogs(opener);
      return;
    }

    const closer = target.closest("[data-dialog-close]");
    if (closer) {
      const dialog = closer.closest("dialog");
      if (dialog) {
        dialog.close();
      }
      return;
    }

    const copy = target.closest("[data-copy-command]");
    if (copy) {
      const dialog = copy.closest("dialog");
      const command = dialog && dialog.querySelector("[data-install-command]");
      const text = command ? command.textContent || "" : "";
      if (text) {
        copyText(text, copy);
      }
    }
  });

  const copyText = async (text, button) => {
    let copied = false;
    if (navigator.clipboard && window.isSecureContext) {
      try {
        await navigator.clipboard.writeText(text);
        copied = true;
      } catch {
        copied = false;
      }
    }
    if (!copied) {
      const field = document.createElement("textarea");
      field.value = text;
      field.setAttribute("readonly", "");
      field.style.position = "fixed";
      field.style.opacity = "0";
      document.body.appendChild(field);
      field.select();
      try {
        copied = document.execCommand("copy");
      } catch {
        copied = false;
      }
      field.remove();
    }
    button.textContent = copied ? "Copied" : "Copy failed";
    if (copied) {
      button.setAttribute("data-copied", "true");
    }
  };

  const bindConfirmDialog = (deleteDialog) => {
    const form = deleteDialog.querySelector("form");
    const input = deleteDialog.querySelector("[data-confirm-input]");
    const submit = deleteDialog.querySelector("[data-confirm-submit]");
    if (form && input && submit) {
      input.addEventListener("input", () => {
        const expected = deleteDialog.getAttribute("data-expected-name") || "";
        const matches = input.value === expected && expected !== "";
        submit.disabled = !matches;
        input.setAttribute("aria-invalid", matches ? "false" : "true");
      });
      form.addEventListener("submit", (event) => {
        const expected = deleteDialog.getAttribute("data-expected-name") || "";
        if (expected === "" || input.value !== expected) {
          event.preventDefault();
          input.setAttribute("aria-invalid", "true");
          input.focus();
        }
      });
    }
  };

  for (const id of ["delete-stack", "delete-container"]) {
    const dialog = document.getElementById(id);
    if (dialog) {
      bindConfirmDialog(dialog);
    }
  }

  const updateStamp = () => {
    document.querySelectorAll("[data-updated]").forEach((element) => {
      const now = new Date();
      element.textContent = "Updated " + now.toLocaleTimeString([], {
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
      });
    });
  };
  document.addEventListener("htmx:afterSwap", updateStamp);
  updateStamp();

  const installWatch = document.querySelector('[data-install-watch="true"]');
  if (installWatch) {
    window.setTimeout(() => window.location.reload(), 4000);
  }

  const passwordToggle = document.querySelector("[data-password-toggle]");
  if (passwordToggle) {
    const password = document.getElementById("password");
    passwordToggle.addEventListener("click", () => {
      if (!(password instanceof HTMLInputElement)) {
        return;
      }
      const showing = password.type === "text";
      password.type = showing ? "password" : "text";
      passwordToggle.setAttribute("aria-pressed", showing ? "false" : "true");
      passwordToggle.setAttribute("aria-label", showing ? "Show password" : "Hide password");
      const icon = passwordToggle.querySelector("i");
      if (icon) {
        icon.classList.toggle("fa-eye", showing);
        icon.classList.toggle("fa-eye-slash", !showing);
      }
    });
  }
})();

/* Dashboard time-series charts: draws canvas[data-chart] elements using the
   series JSON in data-series. Percent charts are fixed 0-100; auto charts
   scale to the data. */
(function () {
  "use strict";
  var COLORS = ["#164f3b", "#d97706", "#3b82f6", "#8b5cf6", "#0891b2"];

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
    var max = 100;
    if (!isPercent) {
      max = 1;
      series.forEach(function (s) {
        s.values.forEach(function (v) {
          if (v > max) max = v;
        });
      });
      max = niceCeil(max * 1.15);
    }
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
    var count = series[0].values.length;
    if (count >= 2) {
      ["-60m", "-45m", "-30m", "-15m", "now"].forEach(function (label, idx) {
        ctx.fillText(label, pad.left + (idx * cw) / 4 - 10, height - 5);
      });
    }
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
  document.body.addEventListener("htmx:afterSwap", drawAll);
})();
