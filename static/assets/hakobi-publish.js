/* Pre-submit subdomain availability; POST remains authoritative. */
"use strict";
{

  function setAvailability(form, state, message) {
    var status = form.querySelector("[data-subdomain-availability-status]");
    var submit = form.querySelector("[data-subdomain-submit]");
    status.textContent = message;
    status.dataset.state = state;
    submit.disabled = !["available", "adoptable", "unknown"].includes(state);
  }

  async function checkAvailability(form) {
    var input = form.querySelector("[data-subdomain-input]");
    var value = input.value.trim();
    var request = (form._availabilityRequest || 0) + 1;
    form._availabilityRequest = request;
    if (!value) {
      setAvailability(form, "invalid", "Enter a subdomain.");
      return;
    }
    setAvailability(form, "checking", "Checking subdomain availability…");
    try {
      var response = await fetch(form.dataset.availabilityUrl + "?value=" + encodeURIComponent(value));
      var data = await response.json();
      if (request !== form._availabilityRequest) return;
      setAvailability(form, data.status, data.message);
    } catch (error) {
      if (request === form._availabilityRequest) {
        setAvailability(form, "unknown", "Could not precheck; Hakobi will verify when you publish.");
      }
    }
  }

  function prepareForm(form) {
    if (form.dataset.availabilityBound) return;
    form.dataset.availabilityBound = "true";
    var input = form.querySelector("[data-subdomain-input]");
    input.addEventListener("input", function () {
      window.clearTimeout(form._availabilityTimer);
      form._availabilityTimer = window.setTimeout(function () {
        checkAvailability(form);
      }, 300);
    });
    if (!document.getElementById("publication-message")) checkAvailability(form);
  }

  function prepareForms(root) {
    var scope = root || document;
    scope.querySelectorAll("[data-subdomain-availability-form]").forEach(prepareForm);
    if (scope.matches && scope.matches("[data-subdomain-availability-form]")) prepareForm(scope);
  }

  document.addEventListener("htmx:afterSwap", function (event) {
    prepareForms(event.target);
  });
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", function () { prepareForms(document); });
  } else {
    prepareForms(document);
  }
}
