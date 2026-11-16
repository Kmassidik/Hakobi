/* Boosted SSR forms: pending states, fragment navigation, and notifications. */
"use strict";
{
  var recentNotices = new Map();

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

  function notify(message, kind, throttleMs) {
    if (!message) return;
    var key = (kind || "success") + "\n" + message;
    var now = Date.now();
    if ((recentNotices.get(key) || 0) > now) return;
    recentNotices.set(key, now + (throttleMs || 2000));
    var item = document.createElement("div");
    item.className = "hakobi-toast hakobi-toast-" + (kind || "success");
    item.setAttribute("role", kind === "error" ? "alert" : "status");
    item.textContent = message;
    notificationRegion().appendChild(item);
    window.setTimeout(function () { item.remove(); }, 6000);
  }

  function notifyLocationNotice() {
    var query = new URLSearchParams(window.location.search);
    var message = query.get("notice");
    if (message) notify(message, query.get("status") === "error" ? "error" : "success");
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
      ["/login", "/settings/github/install", "/settings/github/connect"].includes(destination.pathname);
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

  function responseUrl(xhr) {
    try {
      return new URL(xhr.responseURL);
    } catch (e) {
      return null;
    }
  }

  function responseNotice(xhr) {
    var url = responseUrl(xhr);
    if (!url || !url.searchParams.get("notice")) return null;
    return {
      message: url.searchParams.get("notice"),
      kind: url.searchParams.get("status") === "error" ? "error" : "success",
    };
  }

  function cleanMessage(message) {
    return (message || "").replace(/\s+/g, " ").trim().slice(0, 1200);
  }

  function errorMessage(xhr) {
    var body = (xhr.responseText || "").trim();
    if (body) {
      try {
        var parsed = JSON.parse(body);
        if (parsed && typeof parsed.error === "string") return cleanMessage(parsed.error);
      } catch (e) { /* HTML and plain-text responses are handled below. */ }
      if (body.startsWith("<")) {
        var page = new DOMParser().parseFromString(body, "text/html");
        var detail = page.querySelector('[role="alert"], #app-publication, main, body');
        var text = cleanMessage(detail && detail.textContent);
        if (text) return text;
      } else {
        return cleanMessage(body);
      }
    }
    return "Action failed" + (xhr.status ? " (HTTP " + xhr.status + ")" : "") + ". Check the form and try again.";
  }

  function isLoginResponse(xhr) {
    var url = responseUrl(xhr);
    return !!url && url.pathname === "/login";
  }

  document.addEventListener("htmx:beforeSwap", function (event) {
    var xhr = event.detail.xhr;
    var form = xhr._hakobiForm || event.detail.elt;
    if (isLoginResponse(xhr)) {
      event.detail.shouldSwap = false;
      xhr._hakobiLoginRedirect = true;
      window.location.assign(xhr.responseURL);
      return;
    }
    if (!(form instanceof HTMLFormElement)) return;
    var renderedError = xhr.responseText.includes("<main") ||
      xhr.responseText.includes('id="app-publication"');
    if ([400, 422].includes(xhr.status) && renderedError) {
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
    if (form instanceof HTMLElement && form.hasAttribute("hx-get") &&
        form.querySelector('form[aria-busy="true"]')) {
      event.preventDefault();
      return;
    }
    if (form instanceof HTMLFormElement) {
      event.detail.xhr._hakobiForm = form;
      if (!form._pendingButton) form._pendingButton = form.querySelector('[type="submit"]');
      setPending(form, true);
    }
  });

  document.addEventListener("htmx:afterRequest", function (event) {
    var xhr = event.detail.xhr;
    var form = xhr._hakobiForm || event.detail.elt;
    if (!(form instanceof HTMLFormElement)) return;
    setPending(form, false);
    if (xhr._hakobiLoginRedirect || isLoginResponse(xhr)) return;
    var notice = responseNotice(xhr);
    if (event.detail.successful && xhr.status < 400) {
      if (notice) notify(notice.message, notice.kind);
      else if (form.dataset.ssrBoost === "true") notify("Action completed.", "success");
    } else {
      notify(notice ? notice.message : errorMessage(xhr), "error");
    }
  });

  function updatePage(xhr, form) {
    if (xhr._hakobiHistoryDone || !xhr.responseURL) return;
    xhr._hakobiHistoryDone = true;
    var page = new DOMParser().parseFromString(xhr.responseText, "text/html");
    var title = page.querySelector("title");
    if (title) document.title = title.textContent;
    var url = responseUrl(xhr);
    var action = new URL(form.action, window.location.href);
    if (url && url.href !== action.href && url.href !== window.location.href) {
      window.history.pushState({}, "", url.href);
    }
  }

  document.addEventListener("htmx:afterSwap", function (event) {
    prepareForms(event.target);
    var xhr = event.detail.xhr;
    var form = xhr && (xhr._hakobiForm || event.detail.elt);
    if (!xhr || !(form instanceof HTMLFormElement) || form.dataset.ssrBoost !== "true") return;
    if (!xhr._hakobiLoginRedirect && event.detail.successful !== false && xhr.status < 400) {
      updatePage(xhr, form);
    }
  });

  function isPollingElement(element) {
    return element instanceof HTMLElement && /(?:^|,)\s*every\s/.test(element.getAttribute("hx-trigger") || "");
  }

  function reportError(event, network) {
    var element = event.detail.elt;
    var xhr = event.detail.xhr;
    var polling = isPollingElement(element);
    if (polling && (xhr.status === 404 || xhr.status === 410)) {
      element.remove();
      return;
    }
    var message = network ? "Network error. Your action may not have completed." : errorMessage(xhr);
    notify(message, "error", polling ? 60000 : 2000);
  }

  document.addEventListener("htmx:responseError", function (event) { reportError(event, false); });
  document.addEventListener("htmx:sendError", function (event) { reportError(event, true); });
  window.addEventListener("popstate", function () { window.location.reload(); });
  window.addEventListener("pageshow", function () {
    document.querySelectorAll('form[aria-busy="true"]').forEach(function (form) {
      setPending(form, false);
    });
  });

  function init() {
    prepareForms(document);
    notifyLocationNotice();
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
}
