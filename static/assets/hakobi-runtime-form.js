"use strict";
{
  var NAME = /^[A-Za-z_][A-Za-z0-9_]*$/;
  var PROCESS_NAME = /^[a-z0-9-]{1,32}$/;
  var VOLUME_NAME = /^[A-Za-z0-9][A-Za-z0-9_.-]{0,63}$/;

  function fieldName(kind) {
    return kind === "processes" ? "processes_json" : kind === "volumes" ? "volumes_json" : "secrets_json";
  }

  function initialRows(kind, input) {
    var value = JSON.parse(input.value.trim() || (kind === "secrets" ? "{}" : "[]"));
    if (kind === "secrets") {
      if (!value || Array.isArray(value) || typeof value !== "object") throw new Error("Secrets must be an object.");
      return Object.keys(value).map(function (name) { return { name: name, value: value[name] }; });
    }
    if (!Array.isArray(value)) throw new Error(kind + " must be an array.");
    return value;
  }

  function cloneRow(editor, values) {
    var row = editor.template.content.querySelector("[data-runtime-row]").cloneNode(true);
    row.querySelectorAll("[data-field]").forEach(function (input) {
      var value = values[input.dataset.field];
      if (input.dataset.field === "read_only") input.value = String(value === true);
      else input.value = value == null ? "" : String(value);
    });
    return row;
  }

  function render(editor, rows) {
    editor.rows.replaceChildren();
    rows.forEach(function (values) { editor.rows.appendChild(cloneRow(editor, values)); });
    if (!rows.length) editor.rows.appendChild(cloneRow(editor, {}));
    sync(editor, false);
  }

  function rowValues(editor, row) {
    var values = {};
    row.querySelectorAll("[data-field]").forEach(function (input) {
      values[input.dataset.field] = input.dataset.field === "read_only" ? input.value === "true" : input.value;
    });
    return values;
  }

  function validate(editor, values) {
    var name = (values.name || "").trim();
    if (editor.kind === "secrets") {
      if (!NAME.test(name)) return "Secret names must be valid environment variable names.";
      if (editor.form.getAttribute("action") === "/apps" && !values.value) return "New secrets must have values.";
    } else if (!name || (editor.kind === "processes" && !PROCESS_NAME.test(name)) ||
      (editor.kind === "volumes" && !VOLUME_NAME.test(name))) return "Enter a valid " + editor.kind + " name.";
    if (editor.kind === "processes" && (!values.command.trim() || name === "web")) return "Workers need commands and cannot be named web.";
    if (editor.kind === "volumes" && (!values.mount_path.startsWith("/") || values.mount_path === "/" ||
      values.mount_path.includes("..") || values.mount_path.includes(":"))) return "Volume mounts must be clean absolute paths below /.";
    return "";
  }

  function sync(editor, userEdit) {
    var rows = [];
    var names = {};
    var error = "";
    editor.rows.querySelectorAll("[data-runtime-row]").forEach(function (row) {
      var values = rowValues(editor, row);
      if (Object.keys(values).every(function (key) { return values[key] === "" || values[key] === false; })) return;
      error = error || validate(editor, values);
      var name = (values.name || "").trim();
      if (names[name]) error = error || "Names must be unique within this section.";
      names[name] = true;
      if (editor.kind === "secrets") {
        if (!error || NAME.test(name)) values.value = String(values.value || "");
      }
      rows.push(values);
    });
    var output = rows;
    if (editor.kind === "secrets") {
      output = {};
      rows.forEach(function (values) { output[values.name] = values.value; });
    }
    editor.input.value = JSON.stringify(output);
    editor.input.setCustomValidity(error);
    editor.error.textContent = error;
    if (userEdit) editor.form.dispatchEvent(new Event("hakobi:form-change", { bubbles: true }));
    updateReview(editor.form);
    return !error;
  }

  function addRow(editor) {
    var row = cloneRow(editor, {});
    editor.rows.appendChild(row);
    sync(editor, true);
    row.querySelector("[data-field]").focus();
  }

  function prepare(form, wrapper) {
    var kind = wrapper.dataset.runtimeEditor;
    var input = form.elements[fieldName(kind)];
    var editor = {
      form: form, kind: kind, input: input, wrapper: wrapper,
      rows: wrapper.querySelector("[data-runtime-rows]"),
      template: wrapper.querySelector("[data-runtime-template]"),
      error: wrapper.querySelector("[data-runtime-error]")
    };
    if (!input || !editor.rows || !editor.template || !editor.error) return null;
    try { render(editor, initialRows(kind, input)); }
    catch (e) { input.setCustomValidity(e.message); return null; }
    input.hidden = true;
    var raw = form.querySelector("[name=" + fieldName(kind) + "]").closest("[data-runtime-raw]");
    if (raw) raw.hidden = true;
    wrapper.hidden = false;
    var add = form.querySelector("[data-runtime-add=" + kind + "-add]");
    if (add) add.addEventListener("click", function () { addRow(editor); });
    wrapper.addEventListener("input", function (event) {
      if (event.target.closest("[data-runtime-row]")) sync(editor, true);
    });
    wrapper.addEventListener("change", function () { sync(editor, true); });
    wrapper.addEventListener("click", function (event) {
      var button = event.target.closest("[data-runtime-remove]");
      if (!button) return;
      button.closest("[data-runtime-row]").remove();
      if (!editor.rows.children.length) addRow(editor);
      else sync(editor, true);
    });
    return editor;
  }

  function count(form, kind) {
    try {
      var value = JSON.parse(form.elements[fieldName(kind)].value || (kind === "secrets" ? "{}" : "[]"));
      var count = Array.isArray(value) ? value.length : Object.keys(value).length;
      var noun = kind === "processes" ? "worker" : kind === "volumes" ? "volume" : "secret";
      return count + " " + noun + (count === 1 ? "" : "s");
    } catch (e) { return "Invalid " + kind; }
  }

  function updateReview(form) {
    ["secrets", "processes", "volumes"].forEach(function (kind) {
      form.querySelectorAll("[data-review=" + kind + "]").forEach(function (target) {
        target.textContent = count(form, kind);
      });
    });
  }

  function prepareForm(form) {
    if (form.dataset.runtimePrepared) return;
    form.dataset.runtimePrepared = "true";
    var editors = [];
    form.querySelectorAll("[data-runtime-editor]").forEach(function (wrapper) {
      var editor = prepare(form, wrapper);
      if (editor) editors.push(editor);
    });
    form.addEventListener("submit", function (event) {
      if (editors.every(function (editor) { return sync(editor, false); })) return;
      event.preventDefault();
      event.stopImmediatePropagation();
      var invalid = editors.map(function (editor) { return editor.input; }).find(function (input) {
        return input.validationMessage;
      });
      if (invalid) invalid.reportValidity();
    });
    updateReview(form);
  }

  function init(root) {
    var scope = root || document;
    if (scope.matches && scope.matches("[data-app-form]")) prepareForm(scope);
    scope.querySelectorAll("[data-app-form]").forEach(prepareForm);
  }

  document.addEventListener("htmx:afterSwap", function (event) { init(event.target); });
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", function () { init(document); });
  else init(document);
}
