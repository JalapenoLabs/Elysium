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

## SSE connection cap is per-instance (in-memory)

- **Today:** `MAX_SSE_CONNECTIONS_PER_USER` is enforced by an in-memory
  `Arc<Mutex<HashMap>>` in `backend/src/sse.rs`, so the cap holds per backend instance, not
  globally, and the per-user connection counts are not shared across instances.
- **Desired:** track live connections in a shared store (Postgres or Redis) keyed by user, or
  enforce the cap at a shared gateway, so the cap is global regardless of instance count.
- **Unblocks on:** the API running as more than one instance (the worker / horizontal-scale
  architecture in `CLAUDE.md`). Fine while it is single-instance.

## Shared UI primitives live in Elysium instead of `uikit`

- **Today:** the app-wide confirm modal (`frontend/src/gates/ConfirmGate.tsx` +
  `hooks/useConfirm.ts`), the app-wide prompt modal (`frontend/src/gates/PromptGate.tsx` +
  `hooks/usePrompt.ts`), and the `OhLookAtTheTime` timestamp element
  (`frontend/src/components/OhLookAtTheTime.tsx`) are all generic, cross-project UI that
  currently lives inside Elysium.
- **Desired:** move each into `@jalapenolabs/uikit` (the shared design system) so every
  Jalapeno Labs project consumes one implementation, per the "contribute sharable UI back to
  uikit" rule in `CLAUDE.md`. Elysium then imports them from the package.
- **Unblocks on:** nothing external. `uikit` is now consumed from git (the `develop` branch),
  so this is porting the three into the `uikit` repo, releasing them, and swapping Elysium's
  imports over. The confirm/prompt modals need a HeroUI-free restyle to match uikit's own
  design system first.

## `uikit` has no streamlined table kit

- **Today:** settings tables (Sessions, Recent sign-ins) are hand-built `<table>` markup. There
  is no shared, feature-rich table component, so niceties like column sorting, column picking,
  and column reordering would be re-implemented per table.
- **Desired:** `@jalapenolabs/uikit` ships one streamlined `Table` kit with built-in column
  sorting, show/hide column picking, and drag reordering (its `DragNDrop` primitives already
  cover the reordering), which Elysium's tables then adopt.
- **Unblocks on:** building the table kit in the `uikit` repo. It is a `uikit` deliverable, not
  an Elysium one; `uikit` itself is already consumable from git.

## Sessions have no idle timeout or sliding renewal

- **Today:** the `sessions` table (`backend/migrations/0002_sessions.sql`) holds no tokens
  (the session is an opaque id), but the only lifetime control is the 30-day absolute
  `expires_at`; there is no idle timeout and `last_seen_at` is not yet used for renewal.
- **Desired:** idle expiry + sliding renewal driven by `last_seen_at`, honoring the per-org
  `org_auth_policies.session_lifetime_seconds`.
- **Unblocks on:** nothing external; a hardening pass on the session store.
