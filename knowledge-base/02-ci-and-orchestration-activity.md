# CI and Orchestration Activity

Last verified: **2026-11-05**

## Goal

Hakobi needs a normalized activity model for Jenkins, GitLab Runner, and Kubernetes without scraping each vendor’s dashboard or rebuilding its UI.

The useful questions are:

- Which runners exist and who owns them?
- What is running, queued, idle, offline, failing, or stale?
- Which service or deployment does each job affect?
- Is the collector itself healthy?
- What happened before and after a deployment?

## Jenkins

### Surfaces

- Remote HTTP API for jobs, builds, queue, nodes, executors, and build results.
- Prometheus plugin for scrapeable executor, queue, job, build, disk, and node metrics.
- Authentication through API tokens; CSRF and authorization still apply.

Metrics exposed by the Prometheus plugin include:

- `default_jenkins_executors_available`, `_busy`, `_idle`, `_online`, `_defined`, `_connecting`, `_queue_length`
- `default_jenkins_nodes_online`, `default_jenkins_nodes_offline_cause`
- Build success/failure/unstable/aborted counters
- Last-build result, duration, tests, stage result, and log size
- Disk usage per job and Jenkins home

The plugin’s exact prefix is configurable; the default is `default_jenkins_`.

### Activity semantics

| State | Source |
|---|---|
| Queued | Jenkins queue API + `executors_queue_length` |
| Running | Build/executor API |
| Idle capacity | Executor metrics |
| Offline agent | Node API + Metrics Plugin |
| Failing trend | Counters and last-build result |
| Stuck collector | Missing scrape or stale timestamp |

### Hakobi adapter

- Prefer Prometheus scraping if the plugin is installed; otherwise use the Jenkins API at a slower interval.
- Keep both: metrics give trends, the API gives precise current objects and links.
- Do not shell out to `jenkins-cli`; it adds Java/runtime and credential complexity.
- Store the Jenkins base URL, scoped user/token reference, and scrape endpoint—not raw credentials in the browser.

## GitLab Runner

### Surfaces

- Embedded Prometheus metrics server on `/metrics` when `listen_address` is configured.
- Runner business metrics, Go runtime metrics, process metrics, and build version.
- Autoscaling metrics such as machine states and warm-up/creation duration.
- Native Prometheus `ServiceMonitor`/`PodMonitor` support when managed by the Runner Operator or Helm chart.
- GitLab API for jobs, pipelines, runners, and runner status.

### Critical security note

GitLab explicitly states that the runner metrics server has **no authorization** and should not be publicly available. Bind it to loopback or a private monitoring network, or protect it with a proxy/firewall.

### Activity semantics

- Runner online/offline and last-contact time
- Number of currently running jobs
- Concurrency limit and request concurrency
- API failures and job request imbalance
- Autoscaler machine states and creation latency
- Per-runner labels and project scope

### Hakobi adapter

- Treat runner metrics as a private scrape target.
- Correlate `runner_name`/system ID with GitLab project/group ownership.
- Query GitLab for current jobs and pipeline status to avoid showing metrics without context.
- Support GitLab.com, self-managed, and dedicated runners through the same provider interface.

## Kubernetes

Kubernetes does not expose one complete “activity” endpoint. Use complementary sources:

| Source | Answers |
|---|---|
| `kube-state-metrics` | Desired/current object state: pods, deployments, jobs, conditions, ages |
| `metrics-server` | Current CPU/memory usage from kubelet |
| Kubelet `/metrics` and cAdvisor | Container and node resource behavior |
| Kubernetes watch/API | Fast state changes, events, rollouts, job completions |
| Prometheus Operator | Scrape configuration, alerts, service discovery |
| Audit logs | Who changed what |

`kube-state-metrics` generates metrics from Kubernetes API object state, for example pod readiness and deletion timestamps. Metrics questions and alerts can then be expressed in PromQL.

### Activity semantics

- Workload desired replicas vs ready replicas
- Pod restarts, crash loops, pending scheduling, terminating duration
- Job active/succeeded/failed counts
- Deployment rollout progress and generation mismatch
- Node pressure and capacity
- Recent Kubernetes events and warning counts
- Collector freshness and RBAC failures

### Hakobi adapter

- Start read-only with a dedicated ServiceAccount and minimal ClusterRole rules.
- Use watch/informers for live state and periodic reconciliation for drift.
- Do not proxy `kubectl` commands from user input.
- Render intent, not raw YAML, in the primary UI; keep a read-only YAML view for advanced users.
- Treat metrics-server as a live resource view, not historical telemetry.

## Normalized activity model

A provider-neutral model prevents Jenkins, GitLab, and Kubernetes concepts leaking into the Hakobi UI.

```text
ProviderConnection
  id, type, name, endpoint, credential_ref, status, last_seen

Runner
  id, provider, external_id, name, scope, labels
  status: online | offline | stale | unknown
  busy, capacity, queue_length
  last_job_at, last_seen_at, metrics_endpoint_ref

Job
  id, provider, external_id, runner_id
  project_id, service_id
  status: queued | running | succeeded | failed | cancelled | unknown
  started_at, finished_at, duration_ms
  source_url

ActivitySignal
  subject_type, subject_id, kind, severity
  observed_at, value, source, stale_after
```

## Ingestion rules

1. **Read-only first.** Initial connectors must not retry destructive operations automatically.
2. **Collector health is a first-class status.** A quiet runner and a broken connector must not look identical.
3. **Every reading has freshness.** Show `last_seen` and mark stale values.
4. **Do not store unbounded raw payloads.** Persist normalized state and links; retain bounded events for audit.
5. **Separate credentials by provider and scope.** Jenkins tokens and GitLab tokens are highly privileged.
6. **No generic shell executor.** Use typed HTTP clients with allowlisted base URLs.

## Sources

- Jenkins Prometheus plugin metrics: <https://github.com/jenkinsci/prometheus-plugin/blob/master/docs/metrics/index.md>
- Jenkins remote API: <https://www.jenkins.io/doc/book/using/remote-access-api/>
- GitLab Runner monitoring: <https://docs.gitlab.com/runner/monitoring/>
- Kubernetes kube-state-metrics: <https://kubernetes.io/docs/concepts/cluster-administration/kube-state-metrics/>
- Kubernetes metrics-server: <https://github.com/kubernetes-sigs/metrics-server>
