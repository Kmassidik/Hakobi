# Bot Detection and Automatic Banning

Last verified: **2026-11-05**

## Short answer

Hakobi should **orchestrate existing detection and enforcement systems**, not invent bot fingerprinting or instantly ban traffic.

Recommended progression:

1. Observe and label.
2. Recommend actions in shadow mode.
3. Challenge ambiguous traffic.
4. Apply short, reversible bans.
5. Require policy/review for longer bans.

## Detection sources

### Cloudflare

- **Bot score** ranges from 1 (automated) to 99 (human); 0 means not computed.
- Granular bot scores require **Enterprise Bot Management**.
- Pro and higher plans can see grouped bot analytics; lower plans have more limited signals.
- Missing or empty `User-Agent` is assigned score 1. This is a known false-positive source for corporate proxies and Zero Trust/WARP traffic.
- WAF custom rules can act on bot score, but field availability depends on plan.
- Rate limiting rules can block or challenge abusive request rates. Plan limits differ materially: Free has one rule, Pro two, Business five, with advanced counting fields at Enterprise.
- Allowing an IP or ASN can bypass configured custom and rate-limiting rules; allowlists must be deliberate and audited.

Cloudflare is the preferred enforcement point for internet-facing HTTP traffic when the application is already behind Cloudflare.

### CrowdSec

CrowdSec follows a detection/enforcement split:

- A Security Engine produces decisions.
- Local API distributes decisions.
- Remediation Components (“bouncers”) enforce at the network, proxy, WAF, or Cloudflare layer.
- A Cloudflare bouncer can synchronize decisions to Cloudflare.
- Application-layer bouncers support WAF-style protection and virtual patching.

This is a better model than embedding blocking logic into every application handler. CrowdSec is optional; Cloudflare-only deployments can start with WAF/rate-limit rules.

### Application signals

Useful but not sufficient alone:

- Repeated authentication failures
- Abnormal 401/403/429 rates
- Request paths typical of scanners
- Invalid session/token patterns
- CPU or queue pressure
- Geographic or ASN anomalies

Application logs can trigger review but should not alone produce permanent bans.

## Banning architecture

```text
Signals                  Decision                  Enforcement
--------                 --------                  -----------
CF bot/rate/WAF  ─┐
CrowdSec scenarios ├─> normalize → score → policy → Cloudflare rules
Auth failures      │                         ├─> CrowdSec bouncer
Runner anomalies  ─┘                         └─> local firewall
                                      ↓
                            audit + expiry + rollback
```

### Required controls

- **Scope:** IP, CIDR, ASN, country, verified bot, path, header, or fingerprint—depending on provider capability.
- **TTL:** default short; no permanent auto-ban until repeated evidence and a human-approved policy.
- **Allowlist:** admins, office/VPN egress, uptime checks, CI runners, payment webhooks, Cloudflare health checks, verified good bots.
- **Caps:** maximum active bans, maximum new bans per minute, per-tenant limits.
- **Idempotency:** repeated signals must not create duplicate rules.
- **Audit:** actor, detector, evidence, rule, start, expiry, reason, reversal.
- **Rollback:** one-click removal with provider confirmation.
- **Shadow mode:** record what would be banned for a trial period before enforcement.
- **Kill switch:** globally disable automatic enforcement.

### False-positive hazards

- NAT and corporate proxies share public IPs; banning an IP can ban many users.
- CDN/proxy headers are untrusted unless the application is reachable only through a trusted proxy.
- Scanning traffic and verified crawlers are not the same category.
- A Cloudflare IP access allow can bypass other security rules.
- Bot scores can shift after model updates; rules need periodic review.
- Cloudflare rate limit counters are distributed; they are not exact per-origin request accounting.

## Policy model for Hakobi

```text
DetectionPolicy
  scope: zone | service | environment
  signals: []
  condition: expression
  mode: observe | recommend | challenge | ban
  ttl_seconds
  escalation_threshold
  allowlists: []
  require_approval: bool

Decision
  id, policy_id, subject, provider
  action, reason, evidence[]
  starts_at, expires_at
  status: simulated | active | expired | reversed | failed
```

### Safe defaults

| Signal | Initial action |
|---|---|
| Single anomalous bot score | Observe |
| Repeated login failures | Recommend / rate limit |
| High-confidence scanner | Challenge for 15 minutes |
| Sustained attack | Short ban, maximum one hour |
| Repeated evasion | Review for longer ban |
| Verified bot | Never auto-ban; preserve crawler allowlist |

## Opinion

**Do not build Hakobi as a bot-detection engine in the first phase.** Detection quality requires global traffic data that a self-hosted control plane does not have.

Build:

- Provider-agnostic decision records
- Cloudflare and CrowdSec connectors
- Shadow evaluation
- Approvals and audit
- Bounded enforcement

Cloudflare Enterprise Bot Management is expensive and plan-dependent. Hakobi must degrade gracefully when granular scores are unavailable and should not imply protection that the user’s plan does not provide.

## Sources

- Cloudflare bot score: <https://developers.cloudflare.com/bots/concepts/bot-score/>
- Cloudflare bot plans: <https://developers.cloudflare.com/bots/plans/>
- Cloudflare rate limiting rules: <https://developers.cloudflare.com/waf/rate-limiting-rules/>
- Cloudflare IP Access rules: <https://developers.cloudflare.com/waf/tools/ip-access-rules/>
- CrowdSec remediation components: <https://docs.crowdsec.net/u/bouncers/intro/>
- CrowdSec Cloudflare bouncer: <https://docs.crowdsec.net/u/bouncers/cloudflare/>
