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
    form.dispatchEvent(new CustomEvent("hakobi:form-change", { bubbles: true }));
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
