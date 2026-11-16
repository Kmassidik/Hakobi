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

  function sync(picker) {
    var checked = picker.appsBox.querySelector("input:checked");
    if (!checked) return;
    suggestName(picker, checked.value || picker.reposSel.value);
    var dockerfileInput = document.querySelector("[name=github_dockerfile]");
    if (dockerfileInput) dockerfileInput.value = checked.dataset.dockerfile;
    var contextInput = document.querySelector("[name=github_path]");
    if (contextInput) contextInput.value = checked.value;
    loadEnvironment(picker, checked.value);
  }

  async function loadEnvironment(picker, path) {
    var request = (picker.envRequest || 0) + 1;
    picker.envRequest = request;
    picker.envStatus.textContent = "Checking for an example environment file…";
    try {
      var query = "?repo=" + encodeURIComponent(picker.reposSel.value) +
        "&branch=" + encodeURIComponent(picker.branchSel.value) +
        "&path=" + encodeURIComponent(path) +
        "&dockerfile=" + encodeURIComponent(document.querySelector("[name=github_dockerfile]").value);
      var response = await fetch("/api/github/environment" + query);
      var data = await response.json();
      if (!response.ok || request !== picker.envRequest) return;
      if (data.port && !picker.portDirty) {
        document.querySelector("[name=container_port]").value = data.port;
      }
      var keys = Object.keys(data.env || {});
      if (!data.found || !keys.length) {
        picker.envStatus.textContent = data.found
          ? "The example environment file has no valid variable names."
          : "No .env.example or .env.sample found; enter variables manually.";
        return;
      }
      var current = picker.envInput.value.trim();
      if (picker.envDirty || (current && current !== "{}")) {
        picker.envStatus.textContent = "Found " + keys.length + " example keys; kept your existing JSON.";
        return;
      }
      picker.envInput.value = JSON.stringify(data.env, null, 2);
      picker.envStatus.textContent = "Found " + keys.length + " variable names. Add values before deploying.";
    } catch (e) {
      picker.envStatus.textContent = "Could not inspect example environment files; enter variables manually.";
    }
  }

  function renderApps(picker, apps) {
    apps.forEach(function (app, i) {
      var label = document.createElement("label");
      label.className = "flex items-center gap-3 border-b border-line px-1 py-2.5 text-sm last:border-b-0";
      var input = document.createElement("input");
      input.type = "radio";
      input.name = "github_app_path";
      input.value = app.path;
      input.dataset.dockerfile = app.dockerfile;
      if (i === 0) input.checked = true;
      input.addEventListener("change", function () { sync(picker); });
      label.appendChild(input);
      var text = document.createElement("span");
      text.textContent = app.path === "" ? "/ (repo root)" : app.path + "/";
      label.appendChild(text);
      picker.appsBox.appendChild(label);
    });
  }

  async function detect(picker) {
    var repo = picker.reposSel.value;
    var branch = picker.branchSel.value;
    if (!repo || !branch) return;
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
      renderApps(picker, apps);
      setStatus(picker, apps.length === 1 ? "Detected one buildable app." : "Detected " + apps.length + " buildable apps — pick one.");
      sync(picker);
    } catch (e) {
      picker.appsBox.innerHTML = "<p class='m-0 text-xs text-danger'>Scan failed.</p>";
    }
  }

  async function loadBranches(picker, repo) {
    picker.branchSel.innerHTML = "";
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
