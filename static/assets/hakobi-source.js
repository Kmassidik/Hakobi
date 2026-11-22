/* Connected/public source cards for the shared app form. */
"use strict";
{
  function syncCards(form, source) {
    form.querySelectorAll("[data-source-card]").forEach(function (card) {
      var selected = card.dataset.sourceCard === source;
      card.dataset.state = selected ? "selected" : "unselected";
      card.classList.toggle("border-[#165237]", selected);
      card.classList.toggle("bg-[#165237]/5", selected);
      card.classList.toggle("border-zinc-200", !selected);
      card.classList.toggle("bg-white", !selected);
    });
  }

  function syncPanels(form, source) {
    form.querySelectorAll("[data-source-panel]").forEach(function (panel) {
      var enabled = panel.dataset.sourcePanel === source;
      panel.hidden = !enabled;
      panel.querySelectorAll("input, select, textarea").forEach(function (field) {
        field.disabled = !enabled;
      });
    });
  }

  function syncDeploymentPolicy(form, publicSource) {
    var connected = form.querySelector("[data-connected-deploy-policy]");
    var manual = form.querySelector("[data-public-deploy-policy]");
    var checkbox = form.elements.github_auto_deploy;
    if (connected) connected.hidden = publicSource;
    if (manual) manual.hidden = !publicSource;
    if (!checkbox) return;
    if (publicSource) {
      if (!checkbox.disabled) checkbox.dataset.connectedChecked = String(checkbox.checked);
      checkbox.checked = false;
    } else if (checkbox.dataset.connectedChecked) {
      checkbox.checked = checkbox.dataset.connectedChecked === "true";
    }
    checkbox.disabled = publicSource;
  }

  function syncSource(form) {
    var selected = form.querySelector("[name=source_kind]:checked");
    var source = selected ? selected.value : "connected";
    syncCards(form, source);
    syncPanels(form, source);
    syncDeploymentPolicy(form, source === "public");
    var publicEnvironment = form.querySelector("[data-public-environment]");
    if (publicEnvironment) publicEnvironment.hidden = source !== "public";
    form.dispatchEvent(new CustomEvent("hakobi:form-change", { bubbles: true }));
  }

  async function scanEnvironment(form, button) {
    var status = form.querySelector("[data-public-env-status]");
    var repo = form.querySelector('[data-source-panel="public"] [name="github_repo"]').value.trim();
    var branch = form.querySelector('[data-source-panel="public"] [name="github_branch"]').value.trim();
    var path = form.elements.github_path ? form.elements.github_path.value.trim() : "";
    var query = "?repo=" + encodeURIComponent(repo) + "&branch=" + encodeURIComponent(branch) +
      "&path=" + encodeURIComponent(path);
    button.dataset.scanLabel = button.textContent;
    button.textContent = "Scanning repository…";
    button.disabled = true;
    button.setAttribute("aria-busy", "true");
    status.textContent = "Checking .env.example and .env.sample…";
    try {
      var response = await fetch("/api/public/environment" + query);
      var data = await response.json();
      if (!response.ok) throw new Error(data.error || "Public example discovery failed.");
      var keys = Object.keys(data.env || {});
      if (!data.found || !keys.length) {
        status.textContent = data.found ? "No valid variable names found." : "No example environment file found.";
        return;
      }
      var current = JSON.parse(form.elements.env_json.value.trim() || "{}");
      keys.forEach(function (key) {
        if (!Object.prototype.hasOwnProperty.call(current, key)) current[key] = "";
      });
      form.elements.env_json.value = JSON.stringify(current);
      form.dispatchEvent(new CustomEvent("hakobi:env-import"));
      form.dispatchEvent(new CustomEvent("hakobi:form-change", { bubbles: true }));
      status.textContent = "Imported " + keys.length + " names with blank values; existing values were preserved.";
    } catch (error) {
      status.textContent = error.message || "Could not inspect the public repository.";
    } finally {
      button.textContent = button.dataset.scanLabel;
      button.disabled = false;
      button.removeAttribute("aria-busy");
    }
  }

  function prepareForm(form) {
    if (form.dataset.sourcePrepared) return;
    form.dataset.sourcePrepared = "true";
    form.querySelectorAll("[name=source_kind]").forEach(function (radio) {
      radio.addEventListener("change", function () {
        syncSource(form);
        form.dispatchEvent(new CustomEvent("hakobi:source-change", { bubbles: true }));
      });
    });
    var scan = form.querySelector("[data-public-env-scan]");
    if (scan) scan.addEventListener("click", function () { scanEnvironment(form, scan); });
    syncSource(form);
  }

  function init(root) {
    var scope = root || document;
    if (scope.matches && scope.matches("[data-app-form]")) prepareForm(scope);
    scope.querySelectorAll("[data-app-form]").forEach(prepareForm);
  }

  document.addEventListener("htmx:afterSwap", function (event) {
    init(event.target);
  });

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", function () { init(document); });
  } else {
    init(document);
  }
}
