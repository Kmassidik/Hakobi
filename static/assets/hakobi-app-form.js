/* Complete app form: resource presets and JSON validation. */
"use strict";
{

  function init() {
    var form = document.querySelector("[data-app-form]");
    if (!form) return;
    var recommended = form.querySelector("[data-resource-recommended]");
    var custom = form.querySelector("[data-resource-custom]");
    var cpu = form.querySelector("[data-resource-cpu]");
    var memory = form.querySelector("[data-resource-memory]");
    recommended.addEventListener("change", function () {
      cpu.value = "1";
      memory.value = "512";
    });
    [cpu, memory].forEach(function (input) {
      input.addEventListener("input", function () { custom.checked = true; });
    });
    form.addEventListener("submit", function (event) {
      var environment = form.elements.env_json;
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
        form.reportValidity();
      }
    });
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
}
