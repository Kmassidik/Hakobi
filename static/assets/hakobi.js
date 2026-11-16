"use strict";
{

  const prepareInstallDialog = (dialog, button) => {
    const title = dialog.querySelector("[data-install-title]");
    const command = dialog.querySelector("[data-install-command]");
    const copy = dialog.querySelector("[data-copy-command]");
    const label = button.getAttribute("data-install-title") || "this tool";
    const text = button.getAttribute("data-install-command") || "";
    if (title) {
      title.textContent = "Install " + label;
    }
    if (command) {
      command.textContent = text;
    }
    if (copy) {
      copy.textContent = "Copy command";
      copy.removeAttribute("data-copied");
    }
  };

  const prepareDeleteDialog = (dialog, button) => {
    const form = dialog.querySelector("form");
    const name = dialog.querySelector("[data-confirm-name]");
    const input = dialog.querySelector("[data-confirm-input]");
    const submit = dialog.querySelector("[data-confirm-submit]");
    const expected = button.getAttribute("data-delete-name") || "";
    if (form) {
      const action = button.getAttribute("data-delete-action") || "";
      form.setAttribute("action", action);
      if (form.dataset.ssrBoost === "true") {
        form.setAttribute("hx-post", action);
        if (window.htmx) window.htmx.process(form);
      }
    }
    if (name) {
      name.textContent = expected;
    }
    if (input) {
      input.value = "";
      input.setAttribute("aria-invalid", "false");
    }
    if (submit) {
      submit.disabled = true;
    }
    dialog.setAttribute("data-expected-name", expected);
    const resourceName = dialog.querySelector("[data-resource-name]");
    const provider = dialog.querySelector("[data-resource-provider]");
    if (resourceName) resourceName.value = expected;
    if (provider) provider.value = button.getAttribute("data-provider-id") || "";
  };

  const openDialogs = (button) => {
    const id = button.getAttribute("data-dialog-open");
    if (!id) {
      return;
    }
    const dialog = document.getElementById(id);
    if (!dialog) {
      return;
    }
    if (id === "install-tool") {
      prepareInstallDialog(dialog, button);
    }
    if (["delete-stack", "delete-container", "delete-app", "delete-docker-resource"].includes(id)) {
      prepareDeleteDialog(dialog, button);
    }
    dialog.showModal();
  };

  document.addEventListener("click", (event) => {
    const target = event.target instanceof Element ? event.target : null;
    if (!target) {
      return;
    }

    const opener = target.closest("[data-dialog-open]");
    if (opener) {
      openDialogs(opener);
      return;
    }

    const closer = target.closest("[data-dialog-close]");
    if (closer) {
      const dialog = closer.closest("dialog");
      if (dialog) {
        dialog.close();
      }
      return;
    }

    const copy = target.closest("[data-copy-command]");
    if (copy) {
      const dialog = copy.closest("dialog");
      const command = dialog && dialog.querySelector("[data-install-command]");
      const text = command ? command.textContent || "" : "";
      if (text) {
        copyText(text, copy);
      }
    }
  });

  const copyText = async (text, button) => {
    let copied = false;
    if (navigator.clipboard && window.isSecureContext) {
      try {
        await navigator.clipboard.writeText(text);
        copied = true;
      } catch {
        copied = false;
      }
    }
    if (!copied) {
      const field = document.createElement("textarea");
      field.value = text;
      field.setAttribute("readonly", "");
      field.style.position = "fixed";
      field.style.opacity = "0";
      document.body.appendChild(field);
      field.select();
      try {
        copied = document.execCommand("copy");
      } catch {
        copied = false;
      }
      field.remove();
    }
    button.textContent = copied ? "Copied" : "Copy failed";
    if (copied) {
      button.setAttribute("data-copied", "true");
    }
  };

  const confirmationMatches = (dialog) => {
    const input = dialog.querySelector("[data-confirm-input]");
    const submit = dialog.querySelector("[data-confirm-submit]");
    const expected = dialog.getAttribute("data-expected-name") || "";
    const matches = input && input.value === expected && expected !== "";
    if (submit) submit.disabled = !matches;
    if (input) input.setAttribute("aria-invalid", matches ? "false" : "true");
    return matches;
  };

  document.addEventListener("input", (event) => {
    const input = event.target instanceof Element ? event.target.closest("[data-confirm-input]") : null;
    const dialog = input && input.closest("dialog[data-expected-name]");
    if (dialog) confirmationMatches(dialog);
  });

  document.addEventListener("submit", (event) => {
    const dialog = event.target instanceof Element ? event.target.closest("dialog[data-expected-name]") : null;
    if (dialog && !confirmationMatches(dialog)) {
      event.preventDefault();
      event.stopImmediatePropagation();
      const input = dialog.querySelector("[data-confirm-input]");
      if (input) input.focus();
    }
  }, true);

  document.addEventListener("click", (event) => {
    const target = event.target instanceof Element ? event.target : null;
    const tab = target && target.closest("[data-docker-tab]");
    if (!tab) return;
    const selected = tab.getAttribute("data-docker-tab");
    document.querySelectorAll("[data-docker-panel]").forEach((panel) => {
      panel.hidden = panel.getAttribute("data-docker-panel") !== selected;
    });
    document.querySelectorAll("[data-docker-tab]").forEach((button) => {
      button.setAttribute("aria-selected", button === tab ? "true" : "false");
    });
  });

  const syncDockerTab = (url) => {
    const requested = url.searchParams.get("tab");
    if (!requested) return;
    const button = Array.from(document.querySelectorAll("[data-docker-tab]"))
      .find((candidate) => candidate.getAttribute("data-docker-tab") === requested);
    if (button) button.click();
  };

  const updateStamp = () => {
    document.querySelectorAll("[data-updated]").forEach((element) => {
      const now = new Date();
      element.textContent = "Updated " + now.toLocaleTimeString([], {
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
      });
    });
  };
  document.addEventListener("htmx:afterSwap", (event) => {
    updateStamp();
    const target = event.detail.target || event.target;
    if (!(target instanceof Element) || !target.matches("main")) return;
    const responseUrl = event.detail.xhr && event.detail.xhr.responseURL;
    syncDockerTab(new URL(responseUrl || window.location.href, window.location.href));
  });
  updateStamp();
  syncDockerTab(new URL(window.location.href));

  const installWatch = document.querySelector('[data-install-watch="true"]');
  if (installWatch) {
    window.setTimeout(() => {
      const readiness = document.getElementById("overview-readiness");
      if (readiness && window.htmx) window.htmx.trigger(readiness, "refresh");
    }, 4000);
  }

  const passwordToggle = document.querySelector("[data-password-toggle]");
  if (passwordToggle) {
    const password = document.getElementById("password");
    passwordToggle.addEventListener("click", () => {
      if (!(password instanceof HTMLInputElement)) {
        return;
      }
      const showing = password.type === "text";
      password.type = showing ? "password" : "text";
      passwordToggle.setAttribute("aria-pressed", showing ? "false" : "true");
      passwordToggle.setAttribute("aria-label", showing ? "Show password" : "Hide password");
      const icon = passwordToggle.querySelector("i");
      if (icon) {
        icon.classList.toggle("fa-eye", showing);
        icon.classList.toggle("fa-eye-slash", !showing);
      }
    });
  }
}
