/* GitHub source picker: populates repo/branch dropdowns, runs the Dockerfile
   detector, pre-fills path recommendations. Zeroes stay editable. */
"use strict";
{

  function setStatus(picker, text) {
    if (picker.status) picker.status.textContent = text;
  }

  function suggestName(picker, source) {
    if (!picker.nameInput || picker.nameDirty) return;
    var part = source.split("/").filter(Boolean).pop() || "";
    picker.nameInput.value = part.replace(/[^A-Za-z0-9._-]/g, "-").slice(0, 64);
  }

  function notifyFormChange(picker) {
    if (!picker.form || !picker.form.dispatchEvent) return;
    picker.form.dispatchEvent(new CustomEvent("hakobi:form-change", { bubbles: true }));
  }

  function setFieldValue(field, value) {
    if (field) field.value = value;
  }

  function updateAppCards(picker) {
    picker.appsBox.querySelectorAll("label[data-app-option]").forEach(function (label) {
      var input = label.querySelector("input[name=github_app_path]");
      label.dataset.state = input.checked ? "selected" : "unselected";
    });
  }

  function sync(picker) {
    var checked = picker.appsBox.querySelector("input:checked");
    if (!checked) return;
    suggestName(picker, checked.value || picker.reposSel.value);
    var dockerfileInput = picker.form.querySelector("[name=github_dockerfile]");
    var contextInput = picker.form.querySelector("[name=github_path]");
    setFieldValue(dockerfileInput, checked.dataset.dockerfile);
    setFieldValue(contextInput, checked.value);
    updateAppCards(picker);
    notifyFormChange(picker);
    loadEnvironment(picker, checked.value);
  }

  async function loadEnvironment(picker, path) {
    var request = (picker.envRequest || 0) + 1;
    picker.envRequest = request;
    picker.envStatus.textContent = "Checking for an example environment file…";
    try {
      var dockerfile = picker.form.querySelector("[name=github_dockerfile]");
      var query = "?repo=" + encodeURIComponent(picker.reposSel.value) +
        "&branch=" + encodeURIComponent(picker.branchSel.value) +
        "&path=" + encodeURIComponent(path) +
        "&dockerfile=" + encodeURIComponent(dockerfile.value);
      var response = await fetch("/api/github/environment" + query);
      var data = await response.json();
      if (!response.ok || request !== picker.envRequest) return;
      var port = picker.form.querySelector("[name=container_port]");
      if (data.port && !picker.portDirty) setFieldValue(port, data.port);
      var keys = Object.keys(data.env || {});
      if (!data.found || !keys.length) {
        picker.envStatus.textContent = data.found
          ? "The example environment file has no valid variable names."
          : "No .env.example or .env.sample found; enter variables manually.";
        notifyFormChange(picker);
        return;
      }
      var current = picker.envInput.value.trim();
      if (picker.envDirty || (current && current !== "{}")) {
        picker.envStatus.textContent = "Found " + keys.length + " example keys; kept your existing variables.";
        notifyFormChange(picker);
        return;
      }
      picker.envInput.value = JSON.stringify(data.env, null, 2);
      picker.form.dispatchEvent(new CustomEvent("hakobi:env-import"));
      picker.envStatus.textContent = "Found " + keys.length + " variable names. Add values before deploying.";
      notifyFormChange(picker);
    } catch (e) {
      picker.envStatus.textContent = "Could not inspect example environment files; enter variables manually.";
    }
  }

  function appDisplay(app) {
    return app.path === "" ? "/ (repository root)" : app.path;
  }

  function radioInput(picker, app, selected, hidden) {
    var input = document.createElement("input");
    input.type = "radio";
    input.name = "github_app_path";
    input.value = app.path;
    input.dataset.dockerfile = app.dockerfile;
    input.checked = app.path === selected;
    input.className = hidden ? "hidden" : "size-4 shrink-0 accent-[#165237]";
    input.addEventListener("change", function () { sync(picker); });
    return input;
  }

  function renderRadioApps(picker, apps, selected) {
    apps.forEach(function (app) {
      var label = document.createElement("label");
      label.dataset.appOption = "true";
      label.className = "flex cursor-pointer flex-nowrap items-center gap-3 rounded-lg border border-zinc-200 bg-white px-3 py-3 transition has-[:checked]:border-[#165237] has-[:checked]:bg-[#165237]/5";
      var text = document.createElement("span");
      text.className = "min-w-0 flex-1 truncate font-mono text-[13px] text-zinc-800";
      text.textContent = appDisplay(app);
      text.title = appDisplay(app);
      var badge = document.createElement("span");
      badge.className = "shrink-0 rounded-md border border-zinc-200 bg-zinc-50 px-2 py-1 font-mono text-[11px] text-zinc-600";
      badge.textContent = app.dockerfile || "Dockerfile";
      label.append(radioInput(picker, app, selected, false), text, badge);
      picker.appsBox.appendChild(label);
    });
  }

  function selectSearchApp(picker, input) {
    var app = picker.detectedApps.find(function (candidate) {
      return appDisplay(candidate) === input.value;
    });
    picker.appsBox.querySelectorAll("input[name=github_app_path]").forEach(function (radio) {
      radio.checked = !!app && radio.value === app.path;
    });
    var recipe = picker.appsBox.querySelector("[data-gh-selected-recipe]");
    if (recipe) recipe.textContent = app ? app.dockerfile : "Select a detected application";
    if (app) sync(picker);
    else notifyFormChange(picker);
  }

  function searchLabel() {
    var label = document.createElement("span");
    label.className = "mb-2 block text-[13px] font-semibold text-zinc-800";
    label.textContent = "Application";
    return label;
  }

  function renderSearchApps(picker, apps, selected) {
    picker.detectedApps = apps;
    var wrapper = document.createElement("div");
    var input = document.createElement("input");
    input.type = "search";
    input.className = "w-full rounded-lg border border-zinc-200 bg-white px-3 py-2.5 text-[13px] text-zinc-950 outline-none transition placeholder:text-zinc-400 focus:border-[#165237] focus:ring-[3px] focus:ring-[#165237]/10";
    input.placeholder = "Search applications…";
    input.autocomplete = "off";
    input.setAttribute("list", "gh-application-options");
    var selectedApp = apps.find(function (app) { return app.path === selected; });
    input.value = selectedApp ? appDisplay(selectedApp) : "";
    var options = document.createElement("datalist");
    options.id = "gh-application-options";
    var recipe = document.createElement("p");
    recipe.dataset.ghSelectedRecipe = "true";
    recipe.className = "mb-0 mt-2 font-mono text-[11px] text-zinc-500";
    recipe.textContent = selectedApp ? selectedApp.dockerfile : "Select a detected application";
    apps.forEach(function (app) {
      var option = document.createElement("option");
      option.value = appDisplay(app);
      options.appendChild(option);
      picker.appsBox.appendChild(radioInput(picker, app, selected, true));
    });
    input.addEventListener("input", function () { selectSearchApp(picker, input); });
    input.addEventListener("focus", function () { input.select(); });
    wrapper.append(searchLabel(), input, options, recipe);
    picker.appsBox.appendChild(wrapper);
  }

  function renderApps(picker, apps, selectedPath) {
    var selected = apps.some(function (app) { return app.path === selectedPath; })
      ? selectedPath : (apps[0] && apps[0].path);
    if (apps.length > 3) renderSearchApps(picker, apps, selected);
    else renderRadioApps(picker, apps, selected);
    updateAppCards(picker);
  }

  async function detect(picker) {
    var repo = picker.reposSel.value;
    var branch = picker.branchSel.value;
    if (!repo || !branch) return;
    var checked = picker.appsBox.querySelector("input:checked");
    var context = picker.form.querySelector("[name=github_path]");
    var selectedPath = checked ? checked.value : (context ? context.value : "");
    picker.appsBox.innerHTML = "<p class='m-0 text-xs text-muted'>Scanning for Dockerfiles…</p>";
    try {
      var res = await fetch(
        "/api/github/detect?repo=" + encodeURIComponent(repo) + "&branch=" + encodeURIComponent(branch)
      );
      var data = await res.json();
      picker.appsBox.innerHTML = "";
      var apps = data.apps || [];
      if (!apps.length) {
        picker.appsBox.innerHTML =
          "<p class='m-0 rounded-lg bg-amber-50 px-3 py-2 text-xs text-amber-900'>No Dockerfile found at branch head. Add one, or adjust the branch.</p>";
        return;
      }
      renderApps(picker, apps, selectedPath);
      setStatus(picker, apps.length === 1 ? "Detected one buildable app." : "Detected " + apps.length + " buildable apps — pick one.");
      sync(picker);
    } catch (e) {
      picker.appsBox.innerHTML = "<p class='m-0 text-xs text-danger'>Scan failed.</p>";
    }
  }

  async function loadBranches(picker, repo) {
    picker.branchSel.innerHTML = "";
    notifyFormChange(picker);
    try {
      var res = await fetch("/api/github/branches?repo=" + encodeURIComponent(repo));
      var data = await res.json();
      var preferred = picker.reposSel.selectedOptions[0]?.dataset.defaultBranch || "main";
      (data.branches || []).forEach(function (name) {
        var opt = document.createElement("option");
        opt.value = name;
        opt.textContent = name;
        if (name === preferred) opt.selected = true;
        picker.branchSel.appendChild(opt);
      });
      if (!picker.branchSel.value && picker.branchSel.options.length) picker.branchSel.selectedIndex = 0;
      notifyFormChange(picker);
      detect(picker);
    } catch (e) {
      setStatus(picker, "Could not load branches.");
    }
  }

  function populateRepositories(picker, repos) {
    var current = picker.reposSel.value;
    picker.reposSel.innerHTML = "";
    var placeholder = document.createElement("option");
    placeholder.value = "";
    placeholder.textContent = "Pick a repository…";
    picker.reposSel.appendChild(placeholder);
    repos.forEach(function (repo) {
      var opt = document.createElement("option");
      opt.value = repo.full_name;
      opt.dataset.defaultBranch = repo.default_branch;
      opt.textContent = repo.full_name + (repo.private ? " (private)" : "");
      picker.reposSel.appendChild(opt);
    });
    if (current && repos.some(function (repo) { return repo.full_name === current; })) {
      picker.reposSel.value = current;
    }
    notifyFormChange(picker);
  }

  function loadRepositories(picker) {
    fetch("/api/github/status")
      .then(function (r) { return r.json(); })
      .then(function (data) {
        if (!data.app_installed || !data.user_connected) {
          picker.grid.style.display = "none";
          setStatus(
            picker,
            data.app_installed
              ? "Connect your GitHub identity under Settings → GitHub to pick repositories."
              : "Connect GitHub under Settings → GitHub first."
          );
          return;
        }
        setStatus(picker, "Connected as " + (data.github_login || "") + ".");
        return fetch("/api/github/repos")
          .then(function (r) { return r.json(); })
          .then(function (data) {
            populateRepositories(picker, data.repos || []);
            if (data.stale) {
              setStatus(picker, "Showing saved repositories; refreshing from GitHub in the background.");
            }
          });
      })
      .catch(function () { setStatus(picker, "Could not reach GitHub."); });
  }

  function preparePicker(grid) {
    if (grid.dataset.ghPrepared) return;
    grid.dataset.ghPrepared = "true";
    var form = grid.closest("form") || document;
    var picker = {
      grid: grid,
      form: form,
      reposSel: grid.querySelector("[data-gh-repos]"),
      branchSel: grid.querySelector("[data-gh-branches]"),
      appsBox: grid.querySelector("[data-gh-apps]"),
      status: document.querySelector("[data-gh-status]"),
      envInput: form.querySelector("[name=env_json]"),
      envStatus: form.querySelector("[data-gh-env-status]"),
      envDirty: false,
      envRequest: 0,
      nameInput: form.querySelector("[name=name]"),
      nameDirty: false,
      portDirty: false,
    };
    if (!picker.reposSel) return;
    picker.reposSel.addEventListener("change", function () {
      if (picker.reposSel.value) {
        suggestName(picker, picker.reposSel.value);
        loadBranches(picker, picker.reposSel.value);
      }
    });
    picker.branchSel.addEventListener("change", function () { detect(picker); });
    if (picker.nameInput) {
      picker.nameDirty = !!picker.nameInput.value;
      picker.nameInput.addEventListener("input", function () { picker.nameDirty = true; });
    }
    if (picker.envInput) {
      picker.envInput.addEventListener("input", function () { picker.envDirty = true; });
    }
    var portInput = form.querySelector("[name=container_port]");
    if (portInput) {
      portInput.addEventListener("input", function () { picker.portDirty = true; });
    }
    loadRepositories(picker);
  }

  function init(root) {
    var scope = root || document;
    if (scope.matches && scope.matches("[data-gh-picker]")) preparePicker(scope);
    scope.querySelectorAll("[data-gh-picker]").forEach(preparePicker);
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
