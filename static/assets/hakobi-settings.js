"use strict";
{
  function activate(form, tab, focus) {
    var target = form.querySelector('[data-settings-panel="' + tab + '"]') ? tab : "general";
    form.querySelectorAll("[data-settings-panel]").forEach(function (panel) {
      panel.hidden = panel.dataset.settingsPanel !== target;
    });
    form.querySelectorAll("[data-settings-link]").forEach(function (link) {
      var active = link.dataset.settingsLink === target;
      link.dataset.state = active ? "active" : "inactive";
      if (active) link.setAttribute("aria-current", "page");
      else link.removeAttribute("aria-current");
    });
    form.elements.settings_tab.value = target;
    var url = new URL(window.location.href);
    url.searchParams.set("tab", target);
    window.history.replaceState({ tab: target }, "", url.pathname + url.search);
    if (focus) form.querySelector('[data-settings-panel="' + target + '"] h2').focus({ preventScroll: true });
  }

  function setDirty(form, dirty) {
    form.dataset.settingsDirty = dirty ? "true" : "false";
    form.querySelector("[data-settings-dirty]").hidden = !dirty;
    form.querySelector("[data-settings-clean]").hidden = dirty;
  }

  function showInvalid(form) {
    var invalid = Array.from(form.elements).find(function (field) {
      return field.willValidate && field.validationMessage;
    });
    if (!invalid) return;
    var panel = invalid.closest("[data-settings-panel]");
    if (panel) activate(form, panel.dataset.settingsPanel, false);
    invalid.reportValidity();
  }

  function prepare(form) {
    if (form.dataset.settingsPrepared) return;
    form.dataset.settingsPrepared = "true";
    activate(form, form.elements.settings_tab.value, false);
    form.querySelectorAll("[data-settings-link]").forEach(function (link) {
      link.addEventListener("click", function (event) {
        event.preventDefault();
        activate(form, link.dataset.settingsLink, true);
      });
    });
    ["input", "change", "hakobi:form-change"].forEach(function (type) {
      form.addEventListener(type, function (event) {
        if (event.target.name !== "settings_tab") setDirty(form, true);
      });
    });
    form.addEventListener("hakobi:settings-show", function (event) {
      activate(form, event.detail, false);
    });
    form.addEventListener("invalid", function (event) {
      var panel = event.target.closest("[data-settings-panel]");
      if (panel) activate(form, panel.dataset.settingsPanel, false);
    }, true);
    form.addEventListener("submit", function (event) {
      if (form.checkValidity()) return;
      event.preventDefault();
      event.stopImmediatePropagation();
      showInvalid(form);
    });
    form.querySelector("[data-settings-discard]").addEventListener("click", function () {
      setDirty(form, false);
    });
    window.addEventListener("beforeunload", function (event) {
      if (form.dataset.settingsDirty !== "true") return;
      event.preventDefault();
      event.returnValue = "";
    });
    window.addEventListener("popstate", function (event) {
      var tab = event.state && event.state.tab;
      activate(form, tab || new URL(window.location.href).searchParams.get("tab") || "general", false);
    });
  }

  function init(root) {
    var scope = root || document;
    if (scope.matches && scope.matches("[data-settings-form]")) prepare(scope);
    scope.querySelectorAll("[data-settings-form]").forEach(prepare);
  }

  document.addEventListener("htmx:afterSwap", function (event) { init(event.target); });
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", function () { init(document); });
  else init(document);
}
