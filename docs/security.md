# Security posture

What Elysium does to protect accounts and data, and what remains. Auth mechanics
live in `auth-model.md`, authorization in `authz.md`, encryption in
`backend/src/crypto.rs`; this is the consolidated posture and hardening roadmap.

## In place today

- **Passwords never leave the client.** OPAQUE (RFC 9807) aPAKE; the server stores
  an opaque record and never sees the password or a replayable hash. Client-side
  SHA "pass the hash" is explicitly rejected.
- **Second factor.** TOTP (RFC 6238); an enrolled secret is always required to
  finish a password sign-in.
- **Secrets at rest are encrypted.** The OPAQUE server setup and every TOTP secret
  are sealed with XChaCha20-Poly1305 under a required `ELYSIUM_SECRET_KEY`; the
  backend refuses to boot without it (`crypto.rs`). No plaintext-secret fallback.
- **Online-guessing is throttled.** Every verification path (password, TOTP,
  recovery) is rate-limited by a sliding window over the login-event audit trail,
  which doubles as a temporary, self-expiring lockout (`rate_limit.rs`).
- **Authorization is enforced.** Org-scoped mutations run through the RBAC guard
  (`authz.rs`): org edit and invite minting require admin, member management
  requires outranking the target, and no action can grant or reach a role above
  the actor's. Non-membership and insufficient role are the same `403`.
- **Sign-in method policy is enforced.** A password sign-in is refused when no org
  the profile belongs to permits passwords.
- **Accounts are not enumerable.** Method discovery and every auth failure return
  the same shape whether or not the account exists; invite/bootstrap failures are
  generic.
- **Realtime is authorization-scoped.** SSE is session-cookie gated, and events
  reach only org members (org changes) or teammates (presence/profile), resolved
  server-side; a client cannot subscribe to an org it is not in.
- **Sessions.** Opaque id in an `HttpOnly`, `Secure`, `SameSite=Lax` cookie; no
  tokens in the browser; 30-day absolute expiry; single-use instance claim.
- **Baseline response headers.** `X-Content-Type-Options: nosniff`,
  `X-Frame-Options: DENY`, `Referrer-Policy: no-referrer` on every API response.

## Operator's responsibility

TLS termination, HSTS, and the app-origin Content-Security-Policy are the
deployment's responsibility (Elysium does not own hosting; see `CLAUDE.md`).

## Org security controls

Owners/admins configure per-org auth policy (`docs/org-admin.md`): allowed sign-in
methods, mandatory TOTP, and session lifetime. Enforcement is live: a disabled
password method is refused at sign-in; the strictest `session_lifetime_seconds`
across a member's orgs bounds their session; `require_totp` hard-gates a member
without TOTP into enrollment; and an admin can revoke a member's sessions
(force sign-out + live-SSE drop).

## Remaining hardening

- Server-side teeth for `require_totp` (today the enrollment gate is
  client-enforced from the `/context` signal).
- Idle timeout / sliding session renewal (absolute lifetime is enforced).
- Encrypt-at-rest key rotation (the sealed-blob format carries a version tag for
  this).
