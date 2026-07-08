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
