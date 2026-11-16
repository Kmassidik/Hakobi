/* Guided new-app form. Server validation remains authoritative. */
"use strict";
{

  function validateStep(form, step, error) {
    error.textContent = "";
    var panel = form.querySelector('[data-app-step="' + step + '"]');
    var fields = panel.querySelectorAll("input, select, textarea");
    for (var field of fields) {
      if (!field.checkValidity()) {
        field.reportValidity();
        return false;
      }
    }
    if (step === 1 && !panel.querySelector("[name=github_app_path]:checked")) {
      error.textContent = "Select a repository, branch, and detected app before continuing.";
      return false;
    }
    if (step === 3) {
      var environment = form.elements.env_json.value.trim() || "{}";
      try {
        var parsed = JSON.parse(environment);
        if (!parsed || Array.isArray(parsed) || typeof parsed !== "object") throw new Error();
      } catch (e) {
        error.textContent = "Environment variables must be a valid JSON object.";
        form.elements.env_json.focus();
        return false;
      }
    }
    return true;
  }

  function updateReview(form) {
    var values = {
      repo: form.elements.github_repo.value + " · " + form.elements.github_branch.value,
      name: form.elements.name.value || "Not set",
      build: form.elements.build_type.value + " · " + form.elements.github_dockerfile.value,
      port: form.elements.container_port.value || "8080",
    };
    try {
      values.environment = Object.keys(JSON.parse(form.elements.env_json.value || "{}")).length + " variables";
    } catch (e) {
      values.environment = "Invalid JSON";
    }
    Object.keys(values).forEach(function (key) {
      var target = form.querySelector('[data-review="' + key + '"]');
      target.textContent = values[key];
    });
  }

  function showStep(form, step) {
    form.dataset.currentStep = String(step);
    form.querySelectorAll("[data-app-step]").forEach(function (panel) {
      panel.hidden = Number(panel.dataset.appStep) !== step;
    });
    form.querySelectorAll("[data-step-target]").forEach(function (button) {
      var current = Number(button.dataset.stepTarget) === step;
      button.classList.toggle("text-ink", current);
      button.classList.toggle("text-muted", !current);
      if (current) button.setAttribute("aria-current", "step");
      else button.removeAttribute("aria-current");
    });
    form.querySelector("[data-step-back]").hidden = step === 1;
    form.querySelector("[data-step-next]").hidden = step === 4;
    form.querySelector("[data-step-error]").textContent = "";
    if (step === 4) updateReview(form);
    var heading = form.querySelector('[data-app-step="' + step + '"] h2');
    if (heading) {
      heading.tabIndex = -1;
      heading.focus({ preventScroll: true });
    }
  }

  function goToStep(form, target) {
    var current = Number(form.dataset.currentStep);
    var error = form.querySelector("[data-step-error]");
    for (var step = current; step < target; step += 1) {
      if (!validateStep(form, step, error)) return;
    }
    showStep(form, target);
  }

  function init() {
    var form = document.querySelector("[data-app-stepper]");
    if (!form) return;
    form.querySelector("[data-step-progress]").hidden = false;
    form.querySelector("[data-step-navigation]").hidden = false;
    form.querySelector("[data-step-next]").addEventListener("click", function () {
      goToStep(form, Number(form.dataset.currentStep) + 1);
    });
    form.querySelector("[data-step-back]").addEventListener("click", function () {
      showStep(form, Math.max(1, Number(form.dataset.currentStep) - 1));
    });
    form.querySelectorAll("[data-step-target]").forEach(function (button) {
      button.addEventListener("click", function () {
        goToStep(form, Number(button.dataset.stepTarget));
      });
    });
    form.addEventListener("submit", function (event) {
      var error = form.querySelector("[data-step-error]");
      for (var step = 1; step <= 4; step += 1) {
        if (!validateStep(form, step, error)) {
          event.preventDefault();
          showStep(form, step);
          validateStep(form, step, error);
          if (!error.textContent) {
            error.textContent = "Correct the highlighted fields before creating the app.";
          }
          return;
        }
      }
    });
    showStep(form, 1);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
}
