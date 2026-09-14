# Security

## Trust boundary

nginx publishes on `127.0.0.1` only. The API has no authentication yet and its LLM routes write credentials, so
nothing outside this machine may reach it.

The API itself has no published port. Everything reaches it through nginx, which is the
only component that can set `X-Real-IP` and `X-Forwarded-For`. The rate limiter and
the logs trust those headers for that reason alone. Publishing the API port directly
would let a client forge them.

## Rate limiting

Token bucket per client IP (`tower_governor`): 30 requests may be spent at once, refilling at 10 per second. Both
are constants in `api/src/middleware/rate_limit.rs`, not configuration, so every deployment enforces the same
limit. Exceeding it answers `429` with a JSON body, `Retry-After`, and `x-ratelimit-*` headers. Buckets for idle
clients are swept every minute so the key map stays bounded.

## Response headers

The API sets the helmet-equivalent set on every response, including `429`s:

| Header                              | Value                                        |
|-------------------------------------|----------------------------------------------|
| `Content-Security-Policy`           | `default-src 'none'; frame-ancestors 'none'` |
| `Cross-Origin-Opener-Policy`        | `same-origin`                                |
| `Cross-Origin-Resource-Policy`      | `same-origin`                                |
| `Origin-Agent-Cluster`              | `?1`                                         |
| `Referrer-Policy`                   | `no-referrer`                                |
| `Strict-Transport-Security`         | `max-age=63072000; includeSubDomains`        |
| `X-Content-Type-Options`            | `nosniff`                                    |
| `X-DNS-Prefetch-Control`            | `off`                                        |
| `X-Download-Options`                | `noopen`                                     |
| `X-Frame-Options`                   | `DENY`                                       |
| `X-Permitted-Cross-Domain-Policies` | `none`                                       |

nginx adds a lighter set on the SPA (`nosniff`, `DENY`, `Referrer-Policy`,
`Permissions-Policy`) and hides its version. The SPA has no CSP while it is served
by the Vite dev server, which relies on inline scripts and websockets.

## CORS

Explicit allow-list from `CORS_ALLOWED_ORIGINS`, set inline in `compose.yml`. It is empty because the frontend and
API share one origin through nginx, so browsers never need a cross-origin grant.

## Request hygiene

- 30 second handler timeout (`408`).
- 1 MiB body cap in both nginx and the API (`413`).
- Panics in a handler become `500` instead of dropping the connection.
- The API container runs as an unprivileged user.

## Secrets

`.env` is ignored by git, and `.env.example` documents its keys. It holds the infrastructure credentials needed to
bootstrap the stack: Postgres and Redis credentials, `ELYSIUM_ENCRYPTION_KEY`, and optionally `RUST_LOG`.
Application secrets never go there. They are stored in Postgres, encrypted with that key; see `docs/secrets.md`.

## Roadmap

- Authentication on `/api/v1`, then publishing nginx beyond loopback.
- CSP for the SPA once a production static build replaces the dev server.
- TLS termination at nginx.
