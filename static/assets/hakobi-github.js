/* GitHub source picker: populates repo/branch dropdowns, runs the Dockerfile
   detector, pre-fills path recommendations. Zeroes stay editable. */
"use strict";
{

  function setStatus(picker, text) {
    if (picker.status) picker.status.textContent = text;
  }

  function sync(picker) {
    var checked = picker.appsBox.querySelector("input:checked");
    if (!checked) return;
    var dockerfileInput = document.querySelector("[name=github_dockerfile]");
    if (dockerfileInput) dockerfileInput.value = checked.dataset.dockerfile;
  }

  function renderApps(picker, apps) {
    apps.forEach(function (app, i) {
      var label = document.createElement("label");
      label.className = "flex items-center gap-2 rounded-lg border border-line px-3 py-2 text-sm";
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
      (data.branches || []).forEach(function (name) {
        var opt = document.createElement("option");
        opt.value = name;
        opt.textContent = name;
        if (name === "main") opt.selected = true;
        picker.branchSel.appendChild(opt);
      });
      if (!picker.branchSel.value && picker.branchSel.options.length) picker.branchSel.selectedIndex = 0;
      detect(picker);
    } catch (e) {
      setStatus(picker, "Could not load branches.");
    }
  }

  function populateRepositories(picker, repos) {
    picker.reposSel.innerHTML = "";
    var placeholder = document.createElement("option");
    placeholder.value = "";
    placeholder.textContent = "Pick a repository…";
    picker.reposSel.appendChild(placeholder);
    repos.forEach(function (repo) {
      var opt = document.createElement("option");
      opt.value = repo.full_name;
      opt.textContent = repo.full_name + (repo.private ? " (private)" : "");
      picker.reposSel.appendChild(opt);
    });
    picker.reposSel.addEventListener("change", function () {
      if (picker.reposSel.value) loadBranches(picker, picker.reposSel.value);
    });
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
          });
      })
      .catch(function () { setStatus(picker, "Could not reach GitHub."); });
  }

  function init() {
    var grid = document.querySelector("[data-gh-picker]");
    if (!grid) return;
    var picker = {
      grid: grid,
      reposSel: grid.querySelector("[data-gh-repos]"),
      branchSel: grid.querySelector("[data-gh-branches]"),
      appsBox: grid.querySelector("[data-gh-apps]"),
      status: document.querySelector("[data-gh-status]"),
    };
    if (!picker.reposSel) return;
    loadRepositories(picker);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
}
