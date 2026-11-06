# Observability Stack

Last verified: **2026-11-05**

## Boundary decision

Hakobi should **integrate** Prometheus and Grafana, not replace either.

- Prometheus is the metrics system of record.
- Grafana is the visualization and alerting workspace.
- Hakobi is the service/runner/deployment catalog and operational context layer.

Reimplementing a time-series database would add storage, cardinality, query, retention, and high-availability problems without differentiating Hakobi.

## Prometheus

Prometheus is an open-source CNCF monitoring and alerting toolkit. Key properties:

- Multi-dimensional time-series data with labels
- PromQL query language
- Pull-based HTTP scraping, with Pushgateway for short-lived jobs
- Local standalone storage; not dependent on remote storage
- Service discovery and static target configuration
- Alert rules and Alertmanager integration
- Not designed for billing-grade exactness

### Hakobi use

- Query current and historical values through the Prometheus HTTP API.
- Read active alerts and alert state from Alertmanager or the Prometheus API.
- Discover targets with a read-only view of `/api/v1/targets`.
- Keep links to the original Prometheus/Grafana query.
- Cache only short-lived summaries and topology; do not mirror the TSDB.

Potential API endpoints:

```text
GET /api/v1/query
GET /api/v1/query_range
GET /api/v1/alerts
GET /api/v1/targets
GET /api/v1/rules
```

Use a read-only reverse proxy or network policy in front of Prometheus. Never expose an unauthenticated Prometheus instance to the public internet.

## Grafana

Grafana provides dashboards and alerting across many data sources. Grafana Labs commercializes the stack through Grafana Cloud and Enterprise.

As of the snapshot:

- Free Cloud tier with usage limits and 14-day retention
- Pro from **$19/month + usage**, with longer retention and support
- Enterprise from **$25,000/year** minimum commit
- Metrics priced by active series; logs priced by ingest/write/retain

Hakobi should treat Grafana as an external observability workspace:

- Deep-link users to existing dashboards and panels.
- Query Grafana alert state through its API using a scoped service account.
- Optionally provision folders/dashboards later through reviewed API calls.
- Do not iframe dashboards by default; authentication, CSP, and third-party cookie behavior make this fragile.
- Never copy Grafana admin credentials into browser state.

### Grafana integration levels

| Level | Behavior | Risk |
|---|---|---|
| Link only | Health row opens Grafana URL | Lowest |
| Read API | Show alert state and dashboard metadata | Low, requires viewer token |
| Provision | Create/update dashboards and alert rules | Medium; changes user workspace |
| Embed | Render inside Hakobi | Higher; auth/CSP/frame constraints |

Start at **link + read API**. Provisioning is a later, approved action.

## Normalized alert model

```text
ObservationSource
  provider: prometheus | grafana | jenkins | gitlab | cloudflare | kubernetes
  endpoint, credential_ref, last_success, stale_after

Alert
  id, source, external_id, title, severity
  status: firing | pending | resolved | suppressed | unknown
  service_id, environment_id
  started_at, resolved_at
  source_url, labels

HealthSnapshot
  service_id
  status: healthy | degraded | unknown | critical
  evidence[]
  observed_at
```

Rules:

- Unknown is not healthy.
- Every status carries source and timestamp.
- Do not aggregate unrelated alerts into a single “score” without showing evidence.
- Preserve alert identity across polling cycles.
- Deduplicate by provider/external ID, not title.

## What Hakobi can show initially

- Connected Prometheus and Grafana instances
- Scrape target health and last scrape
- Firing alerts mapped to services and environments
- Dashboard links for a service
- Deployment markers correlated with alert changes
- Alert ownership and escalation contact
- Collector freshness and credential failures

## Later, with approval

- Create/maintain dashboards from service templates
- Create alert rules from policy templates
- Silence an alert with bounded duration
- Link an incident or deployment
- Trigger a runbook or annotation

## Failure and security rules

- Fail closed on authorization, fail open on optional enrichment.
- Do not present stale data as current.
- Do not store high-cardinality raw label sets forever.
- Do not send Grafana/Prometheus admin tokens to the browser.
- Use per-connection credentials and explicit read/write capability flags.
- Apply egress allowlists for provider URLs to reduce SSRF risk.

## Sources

- Prometheus overview: <https://prometheus.io/docs/introduction/overview/>
- Prometheus HTTP API: <https://prometheus.io/docs/prometheus/latest/querying/api/>
- Grafana pricing: <https://grafana.com/pricing/>
- Grafana Alerting: <https://grafana.com/docs/grafana/latest/alerting/>
- Grafana service accounts: <https://grafana.com/docs/grafana/latest/administration/service-accounts/>
