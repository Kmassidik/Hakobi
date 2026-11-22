"use strict";
{
  function notify(form) {
    form.dispatchEvent(new CustomEvent("hakobi:form-change", { bubbles: true }));
  }

  function selectedSource(form) {
    var source = form.querySelector("[name=source_kind]:checked");
    return source ? source.value : "connected";
  }

  function setField(form, name, value) {
    var field = form.elements[name];
    if (!field || value == null) return;
    field.value = value;
    field.dispatchEvent(new Event("input", { bubbles: true }));
  }

  function mergeEnvironment(form, environment) {
    var current = JSON.parse(form.elements.env_json.value.trim() || "{}");
    Object.keys(environment || {}).forEach(function (key) {
      if (!Object.prototype.hasOwnProperty.call(current, key)) current[key] = "";
    });
    form.elements.env_json.value = JSON.stringify(current);
    form.dispatchEvent(new CustomEvent("hakobi:env-import"));
  }

  function applyApp(form, app) {
    setField(form, "github_path", app.path);
    setField(form, "github_dockerfile", app.dockerfile || "Dockerfile");
    setField(form, "build_type", app.build_type);
    if (app.container_port) setField(form, "container_port", String(app.container_port));
    if (app.environment_found) mergeEnvironment(form, app.environment);
    notify(form);
  }

  function selectCard(container, label) {
    container.querySelectorAll("label[data-public-app]").forEach(function (card) {
      var selected = card === label;
      card.classList.toggle("border-[#165237]", selected);
      card.classList.toggle("bg-[#165237]/5", selected);
      card.classList.toggle("border-zinc-200", !selected);
    });
  }

  function appCard(form, container, app, selected) {
    var label = document.createElement("label");
    label.dataset.publicApp = app.path;
    label.className = "flex cursor-pointer items-start gap-3 rounded-lg border p-3 " +
      (selected ? "border-[#165237] bg-[#165237]/5" : "border-zinc-200");
    var radio = document.createElement("input");
    radio.type = "radio";
    radio.name = "github_app_path";
    radio.value = app.path;
    radio.checked = selected;
    radio.className = "mt-1 accent-[#165237]";
    var text = document.createElement("span");
    text.className = "text-[12px] font-semibold text-zinc-800";
    text.textContent = (app.path || "/") + " · " + app.build_type +
      (app.container_port ? " · port " + app.container_port : "");
    label.append(radio, text);
    radio.addEventListener("change", function () {
      selectCard(container, label);
      applyApp(form, app);
    });
    return label;
  }

  function renderApps(form, container, apps) {
    container.replaceChildren();
    var selected = apps.find(function (app) { return !app.path; }) || apps[0];
    apps.forEach(function (app) { container.appendChild(appCard(form, container, app, app === selected)); });
    if (selected) applyApp(form, selected);
  }

  function addOptions(select, label, values, current, defaultBranch) {
    if (!values.length) return;
    var group = document.createElement("optgroup");
    group.label = label;
    values.forEach(function (value) {
      var option = document.createElement("option");
      option.value = value;
      option.textContent = value + (value === defaultBranch ? " (default)" : "");
      option.selected = value === current;
      group.appendChild(option);
    });
    select.appendChild(group);
  }

  function renderRefs(select, result, requested) {
    var current = requested || result.default_branch;
    select.replaceChildren();
    addOptions(select, "Branches", result.branches, current, result.default_branch);
    addOptions(select, "Tags", result.tags, current, result.default_branch);
    if (!select.options.length) {
      var option = document.createElement("option");
      option.value = result.default_branch;
      option.textContent = result.default_branch;
      select.appendChild(option);
    }
    select.value = current;
  }

  function availability(form, elements) {
    var active = selectedSource(form) === "public";
    var ready = elements.panel.dataset.publicScanState === "ready";
    elements.scan.disabled = !active || elements.panel.dataset.publicScanState === "pending";
    elements.ref.disabled = !active || !ready;
    if (!ready) elements.refPanel.hidden = true;
  }

  function invalidate(form, elements) {
    if (elements.repo.value.trim() === elements.repo.dataset.scannedUrl) return;
    elements.panel.dataset.publicScanState = "idle";
    elements.refPanel.hidden = true;
    elements.apps.replaceChildren();
    elements.status.textContent = "Repository changed. Scan it before continuing.";
    availability(form, elements);
  }

  async function scan(form, elements, branch) {
    var repo = elements.repo.value.trim();
    elements.panel.dataset.publicScanState = "pending";
    elements.scan.dataset.scanLabel = elements.scan.textContent;
    elements.scan.textContent = branch ? "Scanning selected ref…" : "Scanning repository…";
    elements.scan.disabled = true;
    elements.scan.setAttribute("aria-busy", "true");
    elements.ref.disabled = true;
    elements.apps.replaceChildren();
    elements.status.textContent = "Discovering refs and inspecting a bounded shallow clone…";
    try {
      var response = await fetch("/api/public/scan", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          host_id: form.elements.host_id.value,
          git_url: repo,
          branch: branch || null,
          csrf_token: form.elements.csrf_token.value
        })
      });
      var body = await response.text();
      var result;
      try { result = JSON.parse(body); } catch (_) { result = { error: body }; }
      if (!response.ok) throw new Error(result.error || result.message || "Repository scan failed.");
      elements.repo.dataset.scannedUrl = repo;
      elements.panel.dataset.publicScanState = "ready";
      elements.refPanel.hidden = false;
      renderRefs(elements.ref, result, branch);
      renderApps(form, elements.apps, result.apps || []);
      elements.status.textContent = "Scanned " + result.default_branch + " · " +
        (result.apps || []).length + " detected application(s).";
      notify(form);
    } catch (error) {
      elements.panel.dataset.publicScanState = "idle";
      elements.refPanel.hidden = true;
      elements.status.textContent = error.message || "Repository scan failed.";
    } finally {
      elements.scan.textContent = elements.scan.dataset.scanLabel;
      elements.scan.removeAttribute("aria-busy");
      availability(form, elements);
    }
  }

  function prepare(form) {
    var elements = {
      panel: form.querySelector('[data-source-panel="public"]'),
      repo: form.querySelector("[data-public-repo]"),
      scan: form.querySelector("[data-public-repo-scan]"),
      status: form.querySelector("[data-public-repo-status]"),
      refPanel: form.querySelector("[data-public-ref-panel]"),
      ref: form.querySelector("[data-public-ref]"),
      apps: form.querySelector("[data-public-apps]")
    };
    if (!elements.panel || elements.panel.dataset.publicScanPrepared) return;
    elements.panel.dataset.publicScanPrepared = "true";
    elements.repo.dataset.scannedUrl = elements.panel.dataset.publicScanState === "ready" ? elements.repo.value.trim() : "";
    elements.scan.addEventListener("click", function () { scan(form, elements, null); });
    elements.repo.addEventListener("input", function () { invalidate(form, elements); });
    elements.ref.addEventListener("change", function () { scan(form, elements, elements.ref.value); });
    form.addEventListener("hakobi:source-change", function () { availability(form, elements); });
    availability(form, elements);
  }

  function init(root) {
    var scope = root || document;
    if (scope.matches && scope.matches("[data-app-form]")) prepare(scope);
    scope.querySelectorAll("[data-app-form]").forEach(prepare);
  }

  document.addEventListener("htmx:afterSwap", function (event) { init(event.target); });
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", function () { init(document); });
  else init(document);
}
