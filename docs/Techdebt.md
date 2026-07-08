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

## Session tokens are stored in plaintext and refresh tokens are not rotated

- **Today:** the `sessions` table (`backend/migrations/0002_sessions.sql`) holds the Keycloak
  access, refresh, and id tokens as plaintext, and the BFF does not rotate the refresh token on
  use or proactively refresh the access token behind the cookie.
- **Desired:** encrypt tokens at rest (application-level envelope encryption or Postgres
  `pgcrypto`), rotate the refresh token on each use, and refresh the access token before expiry so
  long-lived sessions stay valid.
- **Unblocks on:** nothing external; a hardening pass on the BFF session store.
