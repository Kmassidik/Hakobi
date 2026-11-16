/* Boosted SSR forms: pending states, fragment navigation, and notifications. */
"use strict";
{

  function notificationRegion() {
    var region = document.getElementById("hakobi-notifications");
    if (!region) {
      region = document.createElement("div");
      region.id = "hakobi-notifications";
      region.className = "hakobi-notifications";
      region.setAttribute("aria-live", "polite");
      region.setAttribute("aria-atomic", "false");
      document.body.appendChild(region);
    }
    return region;
  }

  function notify(message, kind) {
    if (!message) return;
    var item = document.createElement("div");
    item.className = "hakobi-toast hakobi-toast-" + (kind || "success");
    item.setAttribute("role", kind === "error" ? "alert" : "status");
    item.textContent = message;
    notificationRegion().appendChild(item);
    window.setTimeout(function () { item.remove(); }, 6000);
  }

  function setPending(form, pending) {
    var button = form._pendingButton;
    if (!button) return;
    if (pending) {
      button.dataset.originalHtml = button.innerHTML;
      button.dataset.originalDisabled = String(button.disabled);
      button.disabled = true;
      button.setAttribute("aria-busy", "true");
      button.innerHTML = '<span class="hakobi-spinner" aria-hidden="true"></span>Working…';
      form.setAttribute("aria-busy", "true");
    } else {
      button.disabled = button.dataset.originalDisabled === "true";
      button.removeAttribute("aria-busy");
      if (button.dataset.originalHtml) button.innerHTML = button.dataset.originalHtml;
      form.removeAttribute("aria-busy");
      form._pendingButton = null;
    }
  }

  function prepareForm(form) {
    if (form.dataset.ssrPrepared || form.method.toLowerCase() !== "post") return;
    form.dataset.ssrPrepared = "true";
    if (form.hasAttribute("hx-post") || form.dataset.nativeForm !== undefined) return;
    var destination = new URL(form.action, window.location.href);
    var externalFlow = destination.origin !== window.location.origin ||
      ["/settings/github/install", "/settings/github/connect"].includes(destination.pathname);
    if (externalFlow) {
      form.dataset.nativeForm = "";
      return;
    }
    form.dataset.ssrBoost = "true";
    form.setAttribute("hx-post", form.action);
    form.setAttribute("hx-target", "main");
    form.setAttribute("hx-select", "main");
    form.setAttribute("hx-swap", "outerHTML");
    if (window.htmx) window.htmx.process(form);
  }

  function prepareForms(root) {
    var scope = root || document;
    if (scope.matches && scope.matches('main form[method="post"]')) prepareForm(scope);
    scope.querySelectorAll('main form[method="post"]').forEach(prepareForm);
  }

  function responseNotice(xhr) {
    try {
      return new URL(xhr.responseURL).searchParams.get("notice");
    } catch (e) {
      return null;
    }
  }

  function errorMessage(xhr) {
    try {
      return JSON.parse(xhr.responseText).error || "Action failed. Check the form and try again.";
    } catch (e) {
      return "Action failed. Check the form and try again.";
    }
  }

  document.addEventListener("htmx:beforeSwap", function (event) {
    var form = event.detail.elt;
    if (!(form instanceof HTMLFormElement) || form.dataset.ssrBoost !== "true") return;
    if (event.detail.xhr.status === 400 && event.detail.xhr.responseText.includes("<main")) {
      event.detail.shouldSwap = true;
    }
  });

  document.addEventListener("submit", function (event) {
    var form = event.target;
    if (!(form instanceof HTMLFormElement)) return;
    if (form.getAttribute("aria-busy") === "true") {
      event.preventDefault();
      event.stopImmediatePropagation();
      return;
    }
    form._pendingButton = event.submitter || form.querySelector('[type="submit"]');
    if (form.dataset.ssrBoost !== "true" && !form.hasAttribute("hx-post")) {
      if (!event.defaultPrevented) setPending(form, true);
    }
  }, true);

  document.addEventListener("htmx:beforeRequest", function (event) {
    var form = event.detail.elt;
    if (form instanceof HTMLFormElement) {
      if (!form._pendingButton) form._pendingButton = form.querySelector('[type="submit"]');
      setPending(form, true);
    }
  });

  document.addEventListener("htmx:afterRequest", function (event) {
    var form = event.detail.elt;
    var xhr = event.detail.xhr;
    if (form instanceof HTMLFormElement) {
      setPending(form, false);
      if (form.dataset.ssrBoost === "true") {
        var notice = responseNotice(xhr);
        if (event.detail.successful && xhr.status < 400) {
          notify(notice || "Action completed.", "success");
          if (xhr.responseURL) window.history.pushState({}, "", xhr.responseURL);
        } else {
          notify(notice || errorMessage(xhr), "error");
        }
      }
    }
  });

  document.addEventListener("htmx:responseError", function (event) {
    if (!(event.detail.elt instanceof HTMLFormElement)) {
      notify("The server could not complete that action.", "error");
    }
  });
  document.addEventListener("htmx:sendError", function (event) {
    if (!(event.detail.elt instanceof HTMLFormElement)) {
      notify("Network error. Your action may not have completed.", "error");
    }
  });
  window.addEventListener("pageshow", function () {
    document.querySelectorAll('form[aria-busy="true"]').forEach(function (form) {
      setPending(form, false);
    });
  });

  document.addEventListener("htmx:afterSwap", function (event) {
    prepareForms(event.target);
  });

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", function () { prepareForms(document); });
  } else {
    prepareForms(document);
  }
}
