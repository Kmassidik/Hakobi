(() => {
  "use strict";

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
    }

    if (id === "delete-stack" || id === "delete-container") {
      const form = dialog.querySelector("form");
      const name = dialog.querySelector("[data-confirm-name]");
      const input = dialog.querySelector("[data-confirm-input]");
      const submit = dialog.querySelector("[data-confirm-submit]");
      const expected = button.getAttribute("data-delete-name") || "";
      if (form) {
        form.setAttribute(
          "action",
          button.getAttribute("data-delete-action") || "",
        );
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

  const bindConfirmDialog = (deleteDialog) => {
    const form = deleteDialog.querySelector("form");
    const input = deleteDialog.querySelector("[data-confirm-input]");
    const submit = deleteDialog.querySelector("[data-confirm-submit]");
    if (form && input && submit) {
      input.addEventListener("input", () => {
        const expected = deleteDialog.getAttribute("data-expected-name") || "";
        const matches = input.value === expected && expected !== "";
        submit.disabled = !matches;
        input.setAttribute("aria-invalid", matches ? "false" : "true");
      });
      form.addEventListener("submit", (event) => {
        const expected = deleteDialog.getAttribute("data-expected-name") || "";
        if (expected === "" || input.value !== expected) {
          event.preventDefault();
          input.setAttribute("aria-invalid", "true");
          input.focus();
        }
      });
    }
  };

  for (const id of ["delete-stack", "delete-container"]) {
    const dialog = document.getElementById(id);
    if (dialog) {
      bindConfirmDialog(dialog);
    }
  }

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
  document.addEventListener("htmx:afterSwap", updateStamp);
  updateStamp();

  const installWatch = document.querySelector('[data-install-watch="true"]');
  if (installWatch) {
    window.setTimeout(() => window.location.reload(), 4000);
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
})();
