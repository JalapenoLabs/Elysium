# Tech Debt

Known shortcuts and deferred work, tracked so they are not forgotten. Each entry states the
current state, the desired state, and what unblocks it. Remove an entry when it is resolved.

## Default locale is hardcoded to `en-US`

- **Today:** i18next initializes with a fixed `lng: 'en-US'` (`frontend/src/i18n/index.ts`).
  The project ships only `en-US`, so there is nothing to detect between yet.
- **Desired:** once the app supports multiple languages, detect the default locale from the
  user's machine (browser `navigator.language`), falling back to `en-US`, and let a user
  preference override it. `i18next-browser-languagedetector` is the standard fit alongside the
  existing `i18next-http-backend`.
- **Unblocks on:** adding the first non-`en-US` locale bundle under `frontend/public/locales`.

## Auth state-color hexes are hardcoded

- **Today:** the state-color grammar (`accentByState` / `auraSecondaryByState` in
  `frontend/src/components/auth/authFlowState.ts`) hardcodes hex values. If the Tailwind/HeroUI
  theme changes, these do not follow.
- **Desired:** drive these colors from the Tailwind/HeroUI theme so a theme change updates them in
  one place. This is also a prerequisite for per-company theming / white-labeling, where the auth
  colors must match the customer's branding.
- **Unblocks on:** the theming/white-label work landing a themeable color source to bind to.

## SSE connection cap is per-instance (in-memory)

- **Today:** `MAX_SSE_CONNECTIONS_PER_USER` is enforced by an in-memory
  `Arc<Mutex<HashMap>>` in `backend/src/sse.rs`, so the cap holds per backend instance, not
  globally, and the per-user connection counts are not shared across instances.
- **Desired:** track live connections in a shared store (Postgres or Redis) keyed by user, or
  enforce the cap at a shared gateway, so the cap is global regardless of instance count.
- **Unblocks on:** the API running as more than one instance (the worker / horizontal-scale
  architecture in `CLAUDE.md`). Fine while it is single-instance.

## Sessions have no idle timeout or sliding renewal

- **Today:** the `sessions` table (`backend/migrations/0002_sessions.sql`) holds no tokens
  (the session is an opaque id), but the only lifetime control is the 30-day absolute
  `expires_at`; there is no idle timeout and `last_seen_at` is not yet used for renewal.
- **Desired:** idle expiry + sliding renewal driven by `last_seen_at`, honoring the per-org
  `org_auth_policies.session_lifetime_seconds`.
- **Unblocks on:** nothing external; a hardening pass on the session store.

## TOTP secrets and the OPAQUE server setup are stored unencrypted at rest

- **Today:** `user_totp_secrets.secret` and `opaque_server_setup.setup` are plaintext `BYTEA`.
  A database read exposes TOTP secrets (enabling code generation) and the OPAQUE server's
  OPRF seed. The OPAQUE password *records* are not password-equivalent (that is the point of
  OPAQUE), but the TOTP secrets are second-factor-equivalent.
- **Desired:** application-level envelope encryption (a KEK from the environment / KMS) or
  Postgres `pgcrypto` for the TOTP secrets and the OPAQUE setup, decrypted only in memory.
- **Unblocks on:** nothing external; a key-management decision (where the KEK lives).
