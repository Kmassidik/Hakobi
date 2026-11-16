/* Complete app form: resource presets and JSON validation. */
"use strict";
{
  function validateEnvironment(form, event) {
    var environment = form.elements.env_json;
    if (!environment) return;
    environment.setCustomValidity("");
    try {
      var parsed = JSON.parse(environment.value.trim() || "{}");
      if (!parsed || Array.isArray(parsed) || typeof parsed !== "object") {
        throw new Error("not an object");
      }
    } catch (e) {
      environment.setCustomValidity("Environment variables must be a valid JSON object.");
    }
    if (!form.checkValidity()) {
      event.preventDefault();
      event.stopImmediatePropagation();
      form.reportValidity();
    }
  }

  function prepareForm(form) {
    if (form.dataset.appFormPrepared) return;
    form.dataset.appFormPrepared = "true";
    var recommended = form.querySelector("[data-resource-recommended]");
    var custom = form.querySelector("[data-resource-custom]");
    var cpu = form.querySelector("[data-resource-cpu]");
    var memory = form.querySelector("[data-resource-memory]");
    if (recommended && cpu && memory) {
      recommended.addEventListener("change", function () {
        cpu.value = "1";
        memory.value = "512";
      });
    }
    [cpu, memory].forEach(function (input) {
      if (input && custom) {
        input.addEventListener("input", function () { custom.checked = true; });
      }
    });
    form.addEventListener("submit", function (event) {
      validateEnvironment(form, event);
    });
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
