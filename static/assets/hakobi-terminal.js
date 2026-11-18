/* Authenticated build-log replay and terminal controls. */
"use strict";
{
  const terminals = new WeakMap();
  const finalStatuses = new Set(["running", "failed", "cancelled", "rolled_back"]);

  function required(root, selector) {
    return root.querySelector(selector);
  }

  function initialize(root) {
    if (terminals.has(root)) return;
    const output = required(root, "[data-terminal-output]");
    if (!output) return;
    const state = {
      root,
      output,
      status: required(root, "[data-terminal-status]"),
      elapsed: required(root, "[data-terminal-elapsed]"),
      connection: required(root, "[data-terminal-connection]"),
      followButton: required(root, "[data-terminal-follow]"),
      resumeButton: required(root, "[data-terminal-resume]"),
      expandButton: required(root, "[data-terminal-expand]"),
      cancel: required(root, "[data-terminal-cancel]"),
      seen: new Set(),
      follow: true,
      finished: finalStatuses.has(root.dataset.status),
      source: null,
      timer: null,
      expanded: false,
      previousOverflow: "",
      abort: new AbortController(),
    };
    output.querySelectorAll("[data-log-id]").forEach(function (line) {
      state.seen.add(line.dataset.logId);
    });
    terminals.set(root, state);
    bindControls(state);
    updateElapsed(state);
    scrollToEnd(state);
    if (!state.finished) startClock(state);
    if (!state.finished || root.dataset.autoOpen === "true") connect(state);
    else setConnection(state, "History", "history");
  }

  function bindControls(state) {
    const options = { signal: state.abort.signal };
    state.output.addEventListener("scroll", function () {
      const distance = state.output.scrollHeight - state.output.scrollTop - state.output.clientHeight;
      if (distance > 24 && state.follow) setFollow(state, false);
    }, options);
    if (state.followButton) state.followButton.addEventListener("click", function () {
      setFollow(state, !state.follow);
      if (state.follow) scrollToEnd(state);
    }, options);
    if (state.resumeButton) state.resumeButton.addEventListener("click", function () {
      setFollow(state, true);
      scrollToEnd(state);
    }, options);
    if (state.expandButton) state.expandButton.addEventListener("click", function () {
      setExpanded(state, !state.expanded);
    }, options);
    document.addEventListener("keydown", function (event) {
      if (event.key === "Escape" && state.expanded) setExpanded(state, false);
    }, options);
  }

  function setFollow(state, follow) {
    state.follow = follow;
    if (state.followButton) state.followButton.setAttribute("aria-pressed", String(follow));
    if (state.resumeButton) state.resumeButton.hidden = follow;
  }

  function scrollToEnd(state) {
    state.output.scrollTop = state.output.scrollHeight;
  }

  function setExpanded(state, expanded) {
    state.expanded = expanded;
    state.root.classList.toggle("build-terminal--expanded", expanded);
    if (state.expandButton) {
      state.expandButton.setAttribute("aria-pressed", String(expanded));
      state.expandButton.setAttribute("aria-label", expanded ? "Restore build output" : "Expand build output");
      state.expandButton.textContent = expanded ? "Restore" : "Expand";
    }
    if (expanded) {
      state.previousOverflow = document.body.style.overflow;
      document.body.style.overflow = "hidden";
    } else {
      document.body.style.overflow = state.previousOverflow;
    }
    if (state.follow) scrollToEnd(state);
  }

  function connect(state) {
    if (state.source) return;
    setConnection(state, state.finished ? "Replaying" : "Connecting", "connecting");
    const id = encodeURIComponent(state.root.dataset.deploymentId || "");
    const source = new EventSource("/deployments/" + id + "/events");
    state.source = source;
    source.addEventListener("log", function (event) { appendEvent(state, event); });
    source.addEventListener("status", function (event) { statusEvent(state, event); });
    source.onopen = function () {
      if (!state.finished) setConnection(state, "Live", "live");
    };
    source.onerror = function () {
      if (state.finished) {
        source.close();
        state.source = null;
        setConnection(state, "Complete", "closed");
      } else {
        // EventSource reconnects automatically and sends the last log id.
        setConnection(state, "Reconnecting", "reconnecting");
      }
    };
  }

  function appendEvent(state, event) {
    if (event.lastEventId) state.lastEventId = event.lastEventId;
    if (state.seen.has(event.lastEventId)) return;
    let data;
    try { data = JSON.parse(event.data); } catch (error) { return; }
    if (!data || typeof data.message !== "string") return;
    if (event.lastEventId) state.seen.add(event.lastEventId);
    appendLine(state, data);
  }

  function appendLine(state, data) {
    const empty = required(state.root, "[data-terminal-empty]");
    if (empty) empty.remove();
    const line = document.createElement("div");
    line.className = "build-terminal__line";
    if (state.lastEventId) line.dataset.logId = state.lastEventId;
    const timestamp = document.createElement("span");
    timestamp.className = "build-terminal__timestamp";
    timestamp.textContent = String(data.timestamp || "") + " ";
    const level = document.createElement("span");
    level.className = levelClass(data.level);
    level.textContent = String(data.level || "info");
    line.appendChild(timestamp);
    line.appendChild(level);
    line.appendChild(document.createTextNode(" " + data.message));
    state.output.appendChild(line);
    if (state.follow) scrollToEnd(state);
  }

  function levelClass(level) {
    if (level === "error") return "build-terminal__level build-terminal__level--error";
    if (level === "warn") return "build-terminal__level build-terminal__level--warn";
    return "build-terminal__level build-terminal__level--info";
  }

  function statusEvent(state, event) {
    let data;
    try { data = JSON.parse(event.data); } catch (error) { return; }
    if (!data || typeof data.status !== "string") return;
    state.root.dataset.status = data.status;
    updateTimestamps(state, data);
    state.finished = finalStatuses.has(data.status);
    updateStatus(state, data.status);
    updateElapsed(state);
    if (state.finished) {
      stopClock(state);
      if (state.cancel) state.cancel.hidden = true;
      setConnection(state, "Finalizing", "closed");
    } else {
      startClock(state);
    }
  }

  function updateTimestamps(state, data) {
    const started = Date.parse(data.started_at || "");
    const completed = Date.parse(data.completed_at || "");
    if (Number.isFinite(started)) state.root.dataset.startedAtMs = String(started);
    if (Number.isFinite(completed)) state.root.dataset.completedAtMs = String(completed);
    else if (!state.finished) state.root.dataset.completedAtMs = "";
  }

  function updateStatus(state, status) {
    if (!state.status) return;
    state.status.textContent = status;
    const modifier = status === "running" ? "running" :
      (status === "pending" || status === "building" || status === "deploying") ? "active" :
      (status === "failed" || status === "cancelled") ? "failed" : "muted";
    state.status.className = "build-terminal__status build-terminal__status--" + modifier;
  }

  function updateElapsed(state) {
    if (!state.elapsed) return;
    const started = Number(state.root.dataset.startedAtMs || 0);
    const completed = Number(state.root.dataset.completedAtMs || 0);
    if (!started) { state.elapsed.textContent = "00:00"; return; }
    const end = state.finished && completed ? completed : Date.now();
    state.elapsed.textContent = formatDuration(Math.max(0, end - started));
  }

  function formatDuration(milliseconds) {
    const seconds = Math.floor(milliseconds / 1000);
    const pad = function (value) { return String(value).padStart(2, "0"); };
    if (seconds >= 3600) return Math.floor(seconds / 3600) + ":" + pad(Math.floor(seconds / 60) % 60) + ":" + pad(seconds % 60);
    return pad(Math.floor(seconds / 60)) + ":" + pad(seconds % 60);
  }

  function startClock(state) {
    if (state.timer) return;
    state.timer = window.setInterval(function () { updateElapsed(state); }, 1000);
  }

  function stopClock(state) {
    if (state.timer) window.clearInterval(state.timer);
    state.timer = null;
  }

  function setConnection(state, label, kind) {
    state.root.dataset.connection = kind;
    if (!state.connection) return;
    const dot = state.connection.querySelector(".build-terminal__connection-dot");
    state.connection.textContent = "";
    if (dot) state.connection.appendChild(dot);
    state.connection.appendChild(document.createTextNode(label));
  }

  function cleanup(root) {
    const state = terminals.get(root);
    if (!state) return;
    if (state.source) state.source.close();
    stopClock(state);
    state.abort.abort();
    if (state.expanded) document.body.style.overflow = state.previousOverflow;
    terminals.delete(root);
  }

  function scan(scope) {
    if (!(scope instanceof Element || scope instanceof Document)) return;
    if (scope instanceof Element && scope.matches("[data-build-terminal]")) initialize(scope);
    scope.querySelectorAll("[data-build-terminal]").forEach(initialize);
  }

  function cleanupTree(scope) {
    if (!(scope instanceof Element)) return;
    if (scope.matches("[data-build-terminal]")) cleanup(scope);
    scope.querySelectorAll("[data-build-terminal]").forEach(cleanup);
  }

  document.addEventListener("htmx:beforeSwap", function (event) { cleanupTree(event.target); });
  document.addEventListener("htmx:beforeCleanup", function (event) { cleanupTree(event.target); });
  document.addEventListener("htmx:afterSwap", function (event) { scan(event.target); });
  document.addEventListener("htmx:load", function (event) { scan(event.target); });
  window.addEventListener("pagehide", function () {
    document.querySelectorAll("[data-build-terminal]").forEach(cleanup);
  });
  window.addEventListener("pageshow", function () { scan(document); });
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", function () { scan(document); });
  } else {
    scan(document);
  }
}
