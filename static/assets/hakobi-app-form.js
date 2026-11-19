/* Guided app creation, resource presets, and environment variable rows. */
"use strict";
{
  var environments = new WeakMap();
  var ENVIRONMENT_NAME = /^[A-Za-z_][A-Za-z0-9_]*$/;

  function parseEnvironment(value) {
    var parsed = JSON.parse(value.trim() || "{}");
    if (!parsed || Array.isArray(parsed) || typeof parsed !== "object") {
      throw new Error("Environment variables must be a JSON object.");
    }
    Object.keys(parsed).forEach(function (key) {
      if (!ENVIRONMENT_NAME.test(key) || typeof parsed[key] !== "string") {
        throw new Error("Environment variables must have valid names and string values.");
      }
    });
    return parsed;
  }

  function environmentError(editor, message, field) {
    editor.error.textContent = message;
    editor.rows.querySelectorAll("[data-env-key], [data-env-value]").forEach(function (input) {
      input.setCustomValidity("");
      input.removeAttribute("aria-invalid");
    });
    if (field) {
      field.setCustomValidity(message);
      field.setAttribute("aria-invalid", "true");
    }
  }

  function cloneRow(editor, key, value) {
    var row = editor.template.content.querySelector("[data-env-row]").cloneNode(true);
    row.querySelector("[data-env-key]").value = key || "";
    row.querySelector("[data-env-value]").value = value || "";
    return row;
  }

  function renderEnvironment(editor, values) {
    var keys = Object.keys(values);
    editor.rows.replaceChildren();
    if (!keys.length) {
      editor.rows.appendChild(cloneRow(editor, "", ""));
    } else {
      keys.forEach(function (key) {
        editor.rows.appendChild(cloneRow(editor, key, values[key]));
      });
    }
    editor.input.setCustomValidity("");
    environmentError(editor, "");
  }

  function readRows(editor) {
    var values = Object.create(null);
    var seen = new Set();
    var message = "";
    var invalidField = null;
    editor.rows.querySelectorAll("[data-env-row]").forEach(function (row) {
      var keyInput = row.querySelector("[data-env-key]");
      var valueInput = row.querySelector("[data-env-value]");
      var key = keyInput.value;
      var value = valueInput.value;
      if (!key && !value) return;
      values[key] = value;
      if (!key || !value) {
        message = "Enter both a name and a value for each variable, or clear the whole row.";
        invalidField = invalidField || (key ? valueInput : keyInput);
      } else if (!ENVIRONMENT_NAME.test(key)) {
        message = "Variable names must start with a letter or underscore and contain only letters, numbers, and underscores.";
        invalidField = invalidField || keyInput;
      } else if (seen.has(key)) {
        message = "Duplicate variable name \"" + key + "\". Use each name only once.";
        invalidField = invalidField || keyInput;
      }
      seen.add(key);
    });
    return { values: values, message: message, field: invalidField };
  }

  function syncEnvironment(editor, userEdit) {
    var result = readRows(editor);
    editor.input.value = JSON.stringify(result.values);
    editor.input.setCustomValidity("");
    environmentError(editor, result.message, result.field);
    if (userEdit) {
      editor.input.dispatchEvent(new Event("input", { bubbles: true }));
    }
    return !result.message;
  }

  function importEnvironment(editor) {
    try {
      renderEnvironment(editor, parseEnvironment(editor.input.value));
    } catch (e) {
      environmentError(editor, "The imported variables could not be read. Check the JSON and try again.");
    }
  }

  function hasEnvironmentControls(editor) {
    var row = editor.template.content && editor.template.content.querySelector("[data-env-row]");
    return row && row.querySelector("[data-env-key]") && row.querySelector("[data-env-value]") &&
      row.querySelector("[data-env-remove]") && editor.wrapper.querySelector("[data-env-add]");
  }

  function prepareEnvironment(form) {
    var input = form.querySelector("[name=env_json][data-env-json]");
    var wrapper = form.querySelector("[data-env-editor]");
    if (!input || !wrapper) return null;
    var editor = {
      form: form,
      input: input,
      wrapper: wrapper,
      rows: wrapper.querySelector("[data-env-rows]"),
      template: wrapper.querySelector("[data-env-row-template]"),
      error: wrapper.querySelector("[data-env-error]"),
    };
    if (!editor.rows || !editor.template || !editor.error) return null;
    if (!hasEnvironmentControls(editor)) return null;
    try {
      renderEnvironment(editor, parseEnvironment(input.value));
    } catch (e) {
      input.setCustomValidity("Environment variables must be a JSON object with valid names and string values.");
      return null;
    }
    input.hidden = true;
    var raw = input.closest("[data-env-raw]");
    if (raw) raw.hidden = true;
    wrapper.hidden = false;
    return editor;
  }

  function addEnvironmentRow(editor) {
    var row = cloneRow(editor, "", "");
    editor.rows.appendChild(row);
    syncEnvironment(editor, true);
    row.querySelector("[data-env-key]").focus();
  }

  function removeEnvironmentRow(editor, button) {
    var row = button.closest("[data-env-row]");
    if (!row) return;
    var next = row.nextElementSibling || row.previousElementSibling;
    row.remove();
    syncEnvironment(editor, true);
    var target = next && next.querySelector("[data-env-key]");
    (target || editor.wrapper.querySelector("[data-env-add]")).focus();
  }

  function bindEnvironment(editor) {
    editor.wrapper.querySelector("[data-env-add]").addEventListener("click", function () {
      addEnvironmentRow(editor);
    });
    editor.rows.addEventListener("input", function (event) {
      if (event.target.closest("[data-env-row]")) syncEnvironment(editor, true);
    });
    editor.rows.addEventListener("click", function (event) {
      var button = event.target.closest("[data-env-remove]");
      if (button) removeEnvironmentRow(editor, button);
    });
    editor.form.addEventListener("hakobi:env-import", function () {
      importEnvironment(editor);
    });
  }

  function validateRawEnvironment(form) {
    var input = form.elements.env_json;
    if (!input) return true;
    input.setCustomValidity("");
    try {
      parseEnvironment(input.value);
      return true;
    } catch (e) {
      input.setCustomValidity("Environment variables must be a JSON object with valid names and string values.");
      return false;
    }
  }

  function syncFormEnvironment(form) {
    var editor = environments.get(form);
    return editor ? syncEnvironment(editor, false) : validateRawEnvironment(form);
  }

  function activeField(form, name) {
    return form.querySelector('[name="' + name + '"]:not(:disabled)');
  }

  function fieldValue(form, name, fallback) {
    var field = activeField(form, name);
    return field && field.value ? field.value : (fallback || "");
  }

  function portMapping(form, containerFallback) {
    var host = fieldValue(form, "requested_host_port", "auto");
    return host + " → " + fieldValue(form, "container_port", containerFallback);
  }

  function environmentLabel(form) {
    try {
      var count = Object.keys(parseEnvironment(form.elements.env_json.value)).length;
      return count + (count === 1 ? " variable" : " variables");
    } catch (e) {
      return "Invalid variables";
    }
  }

  function updateReview(form) {
    var values = {
      repo: fieldValue(form, "github_repo", "Not selected") + " · " + fieldValue(form, "github_branch", "Not selected"),
      name: fieldValue(form, "name", "—"),
      build: fieldValue(form, "build_type", "Not selected") + " · " + fieldValue(form, "github_dockerfile", "—"),
      port: portMapping(form, "—"),
      environment: environmentLabel(form),
    };
    Object.keys(values).forEach(function (key) {
      var target = form.querySelector('[data-review="' + key + '"]');
      if (target) target.textContent = values[key];
    });
  }

  function selectedText(form, name, fallback) {
    var field = form.elements[name];
    if (!field || !field.value) return fallback;
    if (field.dataset.summaryLabel) return field.dataset.summaryLabel;
    if (!field.selectedOptions || !field.selectedOptions.length) return field.value;
    return field.selectedOptions[0].textContent.trim();
  }

  function updateSummary(form) {
    var autoDeploy = form.elements.github_auto_deploy;
    var cpu = fieldValue(form, "cpu_limit", "—");
    var memory = fieldValue(form, "memory_limit_mb", "—");
    var values = {
      step: (form.dataset.currentStep || "1") + "/4",
      name: fieldValue(form, "name", "—"),
      repo: fieldValue(form, "github_repo", "Not selected"),
      branch: fieldValue(form, "github_branch", "Not selected"),
      host: selectedText(form, "host_id", "Not selected").split("·")[0].trim(),
      runtime: selectedText(form, "build_type", "Not selected") + " · " + portMapping(form, "8080"),
      allocation: cpu + " CPU · " + memory + " MiB",
      environment: environmentLabel(form),
      auto_deploy: autoDeploy && autoDeploy.checked ? "Auto-deploy on push" : "Manual deploy only",
    };
    Object.keys(values).forEach(function (key) {
      form.querySelectorAll('[data-summary="' + key + '"]').forEach(function (target) {
        target.textContent = values[key];
      });
    });
  }

  function updateProgress(button, step) {
    var target = Number(button.dataset.stepTarget);
    var state = target < step ? "completed" : (target === step ? "active" : "upcoming");
    button.dataset.stepState = state;
    button.dataset.state = state;
    button.classList.toggle("text-ink", state !== "upcoming");
    button.classList.toggle("text-muted", state === "upcoming");
    if (state === "active") button.setAttribute("aria-current", "step");
    else button.removeAttribute("aria-current");
    ["badge", "title", "bar"].forEach(function (part) {
      var child = button.querySelector("[data-step-" + part + "]");
      if (!child) return;
      child.dataset.stepState = state;
      child.dataset.state = state;
      if (part === "badge") {
        if (!child.dataset.stepNumber) child.dataset.stepNumber = child.textContent.trim();
        child.textContent = state === "completed" ? "✓" : child.dataset.stepNumber;
      }
    });
  }

  function showStep(form, step) {
    form.dataset.currentStep = String(step);
    form.querySelectorAll("[data-app-step]").forEach(function (panel) {
      panel.hidden = Number(panel.dataset.appStep) !== step;
    });
    form.querySelectorAll("[data-step-target]").forEach(function (button) {
      updateProgress(button, step);
    });
    form.querySelectorAll("[data-step-back]").forEach(function (button) {
      button.hidden = step === 1;
    });
    form.querySelectorAll("[data-step-next]").forEach(function (button) {
      button.hidden = step === 4;
    });
    form.querySelector("[data-step-error]").textContent = "";
    if (step === 4) updateReview(form);
    updateSummary(form);
    var heading = form.querySelector('[data-app-step="' + step + '"] h2');
    if (heading) heading.focus({ preventScroll: true });
  }

  function validateSource(form, panel, error, focus) {
    var publicSource = form.elements.source_kind && form.elements.source_kind.value === "public";
    var repo = activeField(form, "github_repo");
    var branch = activeField(form, "github_branch");
    var detected = panel.querySelector("[name=github_app_path]:checked");
    if (repo && repo.value && branch && branch.value && (detected || publicSource)) return true;
    error.textContent = publicSource
      ? "Enter a public repository URL and branch or tag before continuing."
      : "Select a repository, branch, and detected app before continuing.";
    if (focus) {
      var target = !repo.value ? repo : (!branch.value ? branch : panel.querySelector("[data-gh-apps]"));
      if (target) target.focus({ preventScroll: true });
    }
    return false;
  }

  function validateStep(form, step, error, focus) {
    error.textContent = "";
    if (step === 3 && !syncFormEnvironment(form)) {
      var editor = environments.get(form);
      error.textContent = editor ? editor.error.textContent : "Environment variables must be a JSON object with valid names and string values.";
      if (focus) {
        var invalid = form.querySelector('[data-app-step="3"] [data-env-key][aria-invalid="true"], [data-app-step="3"] [data-env-value][aria-invalid="true"]') || form.elements.env_json;
        invalid.reportValidity();
      }
      return false;
    }
    var panel = form.querySelector('[data-app-step="' + step + '"]');
    var fields = panel.querySelectorAll("input, select, textarea");
    for (var field of fields) {
      if (!field.checkValidity()) {
        error.textContent = "Correct the highlighted fields before continuing.";
        if (focus) field.reportValidity();
        return false;
      }
    }
    return step !== 1 || validateSource(form, panel, error, focus);
  }

  function goToStep(form, target) {
    var current = Number(form.dataset.currentStep);
    var error = form.querySelector("[data-step-error]");
    for (var step = current; step < target; step += 1) {
      if (!validateStep(form, step, error, false)) {
        showStep(form, step);
        validateStep(form, step, error, true);
        return;
      }
    }
    showStep(form, target);
  }

  function prepareStepper(form) {
    var progress = form.querySelector("[data-step-progress]");
    var navigation = form.querySelectorAll("[data-step-navigation]");
    if (!progress || !navigation.length || !form.querySelector("[data-step-next]")) return;
    form.noValidate = true;
    progress.hidden = false;
    navigation.forEach(function (footer) { footer.hidden = false; });
    form.querySelectorAll("[data-step-next]").forEach(function (button) {
      button.addEventListener("click", function () {
        goToStep(form, Number(form.dataset.currentStep) + 1);
      });
    });
    form.querySelectorAll("[data-step-back]").forEach(function (button) {
      button.addEventListener("click", function () {
        showStep(form, Math.max(1, Number(form.dataset.currentStep) - 1));
      });
    });
    form.querySelectorAll("[data-step-target]").forEach(function (button) {
      button.addEventListener("click", function () {
        goToStep(form, Number(button.dataset.stepTarget));
      });
    });
    form.querySelectorAll("[data-review-target]").forEach(function (button) {
      button.addEventListener("click", function () {
        goToStep(form, Number(button.dataset.reviewTarget));
      });
    });
    showStep(form, 1);
  }

  function validateSubmission(form, event) {
    if (!form.matches("[data-app-stepper]")) {
      if (!syncFormEnvironment(form) || !form.checkValidity()) {
        event.preventDefault();
        event.stopImmediatePropagation();
        form.reportValidity();
      }
      return;
    }
    var error = form.querySelector("[data-step-error]");
    for (var step = 1; step <= 4; step += 1) {
      if (!validateStep(form, step, error, false)) {
        event.preventDefault();
        event.stopImmediatePropagation();
        showStep(form, step);
        validateStep(form, step, error, true);
        return;
      }
    }
  }

  function syncResourceCards(form, mode) {
    form.querySelectorAll("[data-resource-card]").forEach(function (card) {
      var selected = card.dataset.resourceCard === mode;
      card.dataset.state = selected ? "selected" : "unselected";
      card.dataset.selected = String(selected);
      card.classList.toggle("border-[#165237]", selected);
      card.classList.toggle("bg-[#165237]/5", selected);
      card.classList.toggle("border-zinc-200", !selected);
      card.classList.toggle("bg-white", !selected);
    });
  }

  function lockResources(cpu, memory, locked) {
    [cpu, memory].forEach(function (input) {
      input.readOnly = locked;
      input.classList.toggle("bg-zinc-100", locked);
      input.classList.toggle("text-zinc-500", locked);
      input.classList.toggle("cursor-not-allowed", locked);
      input.classList.toggle("bg-white", !locked);
    });
  }

  function prepareResources(form) {
    var recommended = form.querySelector("[data-resource-recommended]");
    var custom = form.querySelector("[data-resource-custom]");
    var cpu = form.querySelector("[data-resource-cpu]");
    var memory = form.querySelector("[data-resource-memory]");
    if (!recommended || !custom || !cpu || !memory) return;
    var mode = custom.checked ? "custom" : "recommended";
    var manual = { cpu: cpu.value, memory: memory.value };
    function select(next) {
      if (next === "recommended") {
        if (mode === "custom") manual = { cpu: cpu.value, memory: memory.value };
        cpu.value = "1";
        memory.value = "512";
      } else {
        cpu.value = manual.cpu;
        memory.value = manual.memory;
      }
      mode = next;
      lockResources(cpu, memory, mode === "recommended");
      syncResourceCards(form, mode);
    }
    recommended.addEventListener("change", function () { select("recommended"); });
    custom.addEventListener("change", function () { select("custom"); });
    [cpu, memory].forEach(function (input) {
      input.addEventListener("input", function () {
        if (mode !== "custom") {
          custom.checked = true;
          mode = "custom";
          lockResources(cpu, memory, false);
          syncResourceCards(form, mode);
        }
        manual = { cpu: cpu.value, memory: memory.value };
      });
    });
    select(mode);
  }

  function prepareForm(form) {
    if (form.dataset.appFormPrepared) return;
    form.dataset.appFormPrepared = "true";
    prepareResources(form);
    var editor = prepareEnvironment(form);
    if (editor) {
      environments.set(form, editor);
      bindEnvironment(editor);
    }
    if (form.matches("[data-app-stepper]")) prepareStepper(form);
    function refreshSummary() {
      updateSummary(form);
      if (form.dataset.currentStep === "4") updateReview(form);
    }
    ["input", "change", "hakobi:form-change"].forEach(function (type) {
      form.addEventListener(type, refreshSummary);
    });
    updateSummary(form);
    form.addEventListener("submit", function (event) {
      validateSubmission(form, event);
    });
    if (!editor && form.elements.env_json) {
      form.elements.env_json.addEventListener("input", function () {
        form.elements.env_json.setCustomValidity("");
      });
    }
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
