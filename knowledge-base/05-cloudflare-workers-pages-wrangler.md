# Cloudflare Workers, Pages, and Wrangler

Last verified: **2026-11-05**

## Why this belongs in Hakobi

Cloudflare is both an infrastructure provider and an execution platform. Hakobi already uses Cloudflare Tunnel; the next logical visibility layer is edge workloads:

- Workers
- Pages projects and deployments
- Wrangler-managed environments
- Workers for Platforms dispatch namespaces
- Edge logs, metrics, errors, and deployments

## Product model

### Workers

Workers is Cloudflare’s serverless execution environment for JavaScript/Wasm and static assets, with bindings to KV, D1, R2, Durable Objects, Queues, and more.

Current pricing snapshot:

- Free: 100,000 requests/day, 10 ms CPU/invocation
- Workers Paid: **$5/month minimum per account**
- Paid includes 10 million requests/month and 30 million CPU-ms/month; additional requests and CPU are metered
- Static asset requests are free and unlimited
- Pages Functions are billed as Workers

### Pages

Pages deploys static and full-stack applications via Git integration, Direct Upload, or C3. It supports previews, rollbacks, redirects, and Functions.

Cloudflare now states that **Workers supports most Pages use cases and is the primary platform for new projects**. Hakobi should still inventory existing Pages projects; it should not push every new project to Pages by default.

### Wrangler

Wrangler is the Cloudflare Developer Platform CLI. It uses the permissions of the authenticated user or API token and supports create, develop, deploy, configuration, environments, bundling, and programmatic APIs.

Wrangler is a build/deployment tool, not a long-running management server. Hakobi should prefer the Cloudflare REST API for inventory and monitoring and optionally invoke Wrangler for local/CI workflows.

### Workers for Platforms

Workers for Platforms runs untrusted customer or AI-generated code in isolated per-customer Workers. It provides:

- Dispatch namespaces and dynamic dispatch
- Unlimited applications under a platform
- Per-customer CPU/subrequest limits
- Tags for organization
- Cross-namespace observability and export to third-party platforms
- Custom hostnames and bindings

This is relevant only if Hakobi later runs customer code. It is not needed for managing Hakobi’s own dashboard.

## Observability surfaces

Cloudflare Workers exposes:

- Workers Logs (dashboard collection/filtering)
- Real-time logs for development
- Tail Workers for custom filtering, sampling, and transformation
- Workers Logpush to R2/S3 and logging providers
- Traces and OpenTelemetry export
- Issues for grouped recurring failures
- Built-in metrics: requests, errors, CPU time, wall time, execution duration
- Query Builder
- Workers Logs retention and Logpush pricing differ by plan

A management platform should consume summaries and links first, and only stream logs when a user is actively investigating.

## Proposed Cloudflare adapter

```text
CloudflareConnection
  account_id, token_ref, zones[], capabilities

EdgeProject
  kind: worker | pages | dispatch_namespace
  name, environment, routes, domains
  last_deployment_at, deployment_status
  usage_summary, error_rate
  log_retention, source_url

EdgeDeployment
  project_id, deployment_id, version, status
  created_at, author, source, rollback_available
```

### Read-only phase

- List Workers scripts and Pages projects
- Show deployment history and current production version
- Map routes/custom domains
- Show plan, usage, error rate, and recent issues
- Link to Cloudflare logs and analytics
- Detect stale or disabled projects

### Action phase

- Trigger a Pages deployment or Worker deployment
- Roll back a deployment
- Update environment variables/secrets with approval
- Enable/disable a route
- Tail logs for a bounded session

Every mutation needs a scoped token, confirmation, audit entry, and provider-side verification.

## Token and permission model

- One Cloudflare connection per account or tenant.
- Store API tokens encrypted with a key outside the application database.
- Request only the required account/zone scopes.
- Do not log Authorization headers.
- Do not reuse the setup API token; create a separate operational token.
- Record which token performed each action.
- Support revocation and capability probing.

## Product opinion

Cloudflare edge workloads should be a **first-class provider after Docker and Kubernetes basics**, not the first integration.

Reasons:

- The API is mature and maps well to read-only inventory.
- Workers are not containers; forcing them into a container model will produce bad UX.
- Deployment semantics differ (versions, routes, bindings), so the service catalog needs an `edge` workload kind.
- Wrangler should not be executed from a web request with arbitrary arguments.
- Workers for Platforms is a separate enterprise product, not required for a normal dashboard.

## Sources

- Workers pricing: <https://developers.cloudflare.com/workers/platform/pricing/>
- Workers observability: <https://developers.cloudflare.com/workers/observability/>
- Wrangler: <https://developers.cloudflare.com/workers/wrangler/>
- Pages: <https://developers.cloudflare.com/pages/>
- Pages Functions: <https://developers.cloudflare.com/pages/functions/>
- Workers for Platforms: <https://developers.cloudflare.com/cloudflare-for-platforms/workers-for-platforms/>
- Workers for Platforms observability: <https://developers.cloudflare.com/cloudflare-for-platforms/workers-for-platforms/configuration/observability>
