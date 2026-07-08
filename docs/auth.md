# Authentication & Auth UI

> **Superseded, in part, by `auth-model.md`.** The auth *model* here (Keycloak runs the
> ceremony, browser reaches Keycloak via Keycloakify/reverse-proxy) is retired: the browser now
> never contacts Keycloak, the API is the WebAuthn Relying Party, and our SPA renders every
> input. See `auth-model.md` for the authoritative model and API. The **screen inventory,
> cinematic layer, invite/onboarding UX, and i18n** below still apply; only replace "Keycloakify
> ceremony served by Keycloak" with "native SPA screens calling the API".

This document is the scope and design decisions for Elysium's authentication surface: how a
user gets into the product, and every screen involved. It complements `identity.md` (which
covers Keycloak, the realm, tokens, and the identity database schema). Where the two overlap,
`identity.md` owns the backend security model and this document owns the user-facing contract
and UI.

## Principles

- **Keycloak owns security fully.** Keycloak stores and verifies every credential and runs every
  ceremony (passkey and TOTP). Elysium never reimplements credential storage or verification.
- **We own every pixel.** The login UI is Elysium's own React, never a stock Keycloak page and
  never a foreign-branded screen.
- **The browser only ever sees one origin.** Keycloak is deployed behind Elysium's own gateway on
  Elysium's domain, so the browser never contacts a Keycloak-branded host and never leaves the
  app's origin.
- **Tokens never live in the browser.** Elysium's API is a confidential OIDC client that keeps
  tokens server-side and hands the browser only an HttpOnly, Secure, SameSite session cookie. No
  access or refresh token is ever readable by JavaScript.
- **Passwordless from day 0.** A passkey (WebAuthn) is the primary factor; a TOTP code is the
  fallback. No password is ever entered, stored, or reset.
- **Invite-only, with a first-run root.** A fresh instance is claimed by its first (root) user.
  Everyone after that arrives by a copyable signed invite link. There is no open public
  registration.
- **The first handshake feels like Elysium.** Auth is the first thing a user experiences, so it
  previews the product's soul: a living background that speaks the state-color language, an
  ambient Elysia presence, motion, and sound. It is cinematic, not a bare form.

## Architecture

Three deployment facts make "Keycloak owns everything" and "we own the UI, on one origin"
coexist:

1. **Keycloak runs every ceremony** (passkey, TOTP, enrollment, recovery), storing and verifying
   all secrets. It is the WebAuthn Relying Party; the RP ID is Elysium's registrable domain, so
   passkeys are native.
2. **The login UI is Elysium's React, compiled to a Keycloak theme with Keycloakify.** The user
   sees only Elysium.
3. **Keycloak sits behind Elysium's gateway** under the app's own domain (for example
   `app.example.com/auth/*`), reverse-proxied, so the browser only ever contacts one origin.
4. **Token custody is a BFF token-handler.** Elysium's API is a confidential OIDC client. It
   receives the OIDC callback, exchanges the code for tokens, keeps them in a server-side session
   store, and gives the browser only an HttpOnly cookie.

```
Browser ── one origin (app.example.com) ──┬─▶ /          SPA (the React app)
                                          ├─▶ /auth/*     Keycloak, themed with our React (Keycloakify), reverse-proxied
                                          └─▶ /api/*       Elysium API (cookie session + app endpoints)

Elysium API ── confidential OIDC + Admin API ──▶ Keycloak (code exchange, user & credential provisioning)
```

- **The SPA** renders the app, holds a cookie, triggers login by navigating to `/auth/*`
  (Keycloak, proxied), and reads auth state from `GET /api/v1/auth/session`. It never parses a
  token and never holds one.
- **Keycloak** owns the identity directory, all credentials, the login and enrollment flows, and
  is the token authority. Downstream token validation (`backend/src/auth.rs`) is unchanged; it
  validates real Keycloak JWTs, which now travel service-side.
- **This retires** the browser-side `keycloak-js` + public-SPA-client + PKCE-in-browser wiring in
  favor of the confidential-client BFF and the cookie session.

## Two surfaces, one visual language

- **Surface A, the ceremony (Keycloakify).** The credential screens, served by Keycloak under
  `/auth`. Our React, compiled to a theme. Localized with Keycloak/Keycloakify message bundles.
- **Surface B, the app (React Router).** Everything around the ceremony that lives in the product,
  served by the SPA. Localized with `i18next`.

Both share one design system so the seam is invisible.

## The frontend auth API contract

The SPA is built against this small set of API endpoints (the ceremony itself is Keycloak's).
Bodies are protobuf per `transport.md`; the session is a cookie set by the API.

| Endpoint | Purpose |
|---|---|
| `GET /api/v1/auth/context` | Is this instance claimed yet? Drives first-run vs. sign-in. Public. |
| `POST /api/v1/auth/bootstrap` | Claim an unclaimed instance: provision the root user in Keycloak + first org, then hand off to Keycloak credential enrollment. |
| `GET /api/v1/auth/session` | Current principal and auth state. Replaces client-side token parsing. |
| `POST /api/v1/auth/logout` | Clear the session; the API ends the Keycloak session. |
| `POST /api/v1/invites` | Mint a copyable signed invite link (org/team/role/expiry). |
| `GET /api/v1/invites/:token` | Verify a signed invite and preview its org/team. Public. |
| `POST /api/v1/invites/:token/accept` | Provision the Keycloak user + membership, then hand off to enrollment. |
| `GET /api/v1/auth/credentials`, `GET /api/v1/auth/sessions` | Read-only views for the account UI; mutations deep-link into Keycloak's themed account flows. |

Credential mutation (add/remove passkey, reset TOTP, regenerate recovery codes) is a Keycloak
ceremony, surfaced in-app as a deep link into the themed account flow, not reimplemented on our
API.

## Access model: invite-only, first-run root

- **First run.** On a brand-new instance there are zero users. `GET /api/v1/auth/context` reports
  the instance is unclaimed and the app routes to the **Claim Instance** wizard: it provisions the
  root owner in Keycloak, hands off to Keycloak's themed enrollment (first passkey + recovery
  codes), then returns to finish the first organization and profile. Afterward the instance is
  claimed and the wizard never shows again.
- **Steady state.** Registration is invite-only (the realm already has `registrationAllowed` and
  `resetPasswordAllowed` off). An admin mints a **copyable signed invite link**; the invitee opens
  it, the app verifies the signature and previews the org/team, then hands off to Keycloak's themed
  enrollment. No email delivery is required, which suits self-hosted and air-gapped installs.
- **No open signup, no password reset.** There is no self-service public registration and no
  password reset, because there is no password. A lost device is handled by recovery codes and,
  failing that, an admin re-issuing an invite.

### Signed invite links

An invite is a self-contained token signed by Elysium's API, encoding the org, team, role, and
an expiry. It is delivered however the admin likes (copy/paste, chat, email), not tied to SMTP.
`GET /api/v1/invites/:token` verifies signature and expiry before anything is shown; acceptance
provisions the Keycloak user and membership and then enters enrollment. Tokens are single-use and
short-lived.

## Screen inventory

### Surface A, ceremony (Keycloakify templates under `/auth`)

Each maps to a real Keycloakify template, so this is buildable, not hand-wavy. The `State` column
names the ambient background color from Elysium's state grammar.

| Screen | Template | Purpose | State |
|---|---|---|---|
| Identify | `login-username.ftl` | Enter email/username. No password field. | authenticating |
| Passkey challenge | `login-webauthn-authenticate.ftl` | The passkey tap/biometric. Primary path. | needs-input |
| TOTP challenge | `login-otp.ftl` | Six-digit authenticator code. Fallback. | needs-input |
| Passkey enrollment | `webauthn-register.ftl` | Create the first passkey (the passwordless bootstrap). | needs-input → success |
| TOTP enrollment | `login-config-totp.ftl` | QR + verify an authenticator app. | needs-input |
| Recovery codes | `login-recovery-authn-code-config.ftl` / input | Generate, confirm, and later consume one-time recovery codes. | needs-input |
| Error / expired / info | `error.ftl`, `login-page-expired.ftl`, `info.ftl` | Branded terminal states. | error |
| Logout confirm | `logout-confirm.ftl` | Branded sign-out. | idle |

### Surface B, app (React Router routes)

| Screen | Route | Purpose | State |
|---|---|---|---|
| Splash / gate | `/` (pre-auth) | Living landing while the session resolves. First impression. | idle |
| Signing in | transient | Post-callback handoff into the workspace. Cinematic, not a white flash. | authenticating → success |
| Claim instance | `/setup` | First-run root: provision the owner + first org, orchestrate enrollment. | idle → success |
| Onboarding | `/onboarding` | Welcome and profile (avatar, display name, status); wraps the Keycloak enrollment hand-off. | idle |
| Invite acceptance | `/invite/:token` | Verify the signed link, preview the org/team, enter enrollment. | idle → success |
| Invalid / expired invite | `/invite/:token` (fault) | Kind dead-end with a path to request a new invite. | error |
| Signed out | `/signed-out` | Confirmed sign-out, invite back in. | idle |
| Session expired | modal (any route) | Session could not refresh; re-authenticate in place without losing work. | needs-input |
| Security center | `/settings/security` | View passkeys, authenticators, recovery-code status; deep-link into Keycloak to change them. | idle |
| Active sessions | `/settings/sessions` | Devices, locations, last-seen; remote sign-out. | idle |
| Profile settings | `/settings/profile` | Avatar, bio, status, display name. | idle |
| Access denied | `/denied` | Authenticated but lacks the team permission. | error |
| Account suspended | `/suspended` | Brute-force lockout or admin suspension, with the right recovery CTA. | error |
| Identity unavailable | fallback | The API or Keycloak is unreachable. A branded, reassuring degraded state, not a silent anonymous render. | error |

## Passwordless enrollment & recovery

A passwordless system's largest support burden is the lost device. This is designed in, not
bolted on, and it is owned by Keycloak's flow:

- **Enrollment always issues recovery codes.** Whenever a first passkey is created (root claim or
  invite acceptance), Keycloak's enrollment flow generates one-time recovery codes and requires
  the user to confirm they have saved them before continuing.
- **Recovery is a first-class sign-in path.** A lost-device sign-in consumes one recovery code,
  then immediately walks the user through enrolling a fresh passkey.
- **Admin re-invite is the last resort.** If recovery codes are also lost, an admin re-issues a
  signed invite, which resets enrollment. No password ever re-enters the picture.

## The cinematic layer

This is the "bring it to life" work, in scope for v1, and it spans both surfaces so the ceremony
and the app feel like one piece.

- **State-color grammar.** The ambient background shifts with the flow using Elysium's
  vocabulary: calm idle, **orange** while authenticating, **pink** when it needs your passkey tap,
  **red** on failure, a **green** flash on success. Auth is where users first learn this language.
- **Elysia presence.** A small ambient geometry (a nod to the workspace icosahedron) reacts to the
  flow: it pulses while thinking, leans in when it needs a tap, and settles once you are in.
- **Motion.** Framer Motion (already a dependency) drives staged entrances, the passkey "tap"
  pulse, and a success transition that carries the user into the workspace rather than cutting.
- **Sound.** Short, cohesive cues: a soft chime on success, a distinct "your turn" cue when the
  passkey prompt appears (also an accessibility win), all behind a mute preference and
  `prefers-reduced-motion`.
- **Voice (opt-in).** Elysia can speak a brief "Welcome back" on success. Off by default.
- **Copy & tone.** Warm, confident, human. "Tap your passkey to continue," never "WebAuthn
  authentication required." Error copy helps and never blames.
- **Craft.** Full dark/light theming, complete keyboard navigation, screen-reader labels on every
  step, respectful reduced-motion, and fast perceived load via the splash.

Assets required for this layer are specified in `auth-art-briefs.md`.

## Internationalization

- **Surface A (Keycloakify)** localizes with Keycloak/Keycloakify message bundles (one properties
  file per locale).
- **Surface B (SPA)** localizes with `i18next` under an `auth` namespace, `common` for shared
  words.

Both cover the same locale set the project ships; `en-US` is the source. Validation, empty
states, and error copy are translated; logs and identifiers stay English. See `i18n.md`.

## Frontend structure

```
frontend/
  keycloak-theme/            # Keycloakify project: Surface A ceremony templates as our React
    login/
      Login.tsx              # login-username
      LoginWebAuthn.tsx      # passkey challenge
      LoginOtp.tsx           # TOTP challenge
      WebAuthnRegister.tsx   # passkey enrollment
      LoginConfigTotp.tsx    # TOTP enrollment
      RecoveryCodes.tsx      # recovery-code config/input
      Error.tsx, PageExpired.tsx, Info.tsx, LogoutConfirm.tsx
    shared/                  # AmbientBackground, ElysiaPresence, sound, tokens (shared with the app)
  src/
    auth/
      session.ts             # session state (GET /auth/session), replaces client token parsing
      useSession.ts          # hook exposing principal + status
      RequireAuth.tsx        # route guard; preserves intended destination
      RequireInstance.tsx    # routes to /setup when the instance is unclaimed
    pages/
      Auth/Splash.tsx, SigningIn.tsx, SignedOut.tsx
      Setup/ClaimInstance.tsx
      Onboarding/Welcome.tsx, ProfileStep.tsx
      Invite/InviteAcceptance.tsx, InvalidInvite.tsx
      Settings/SecurityCenter.tsx, ActiveSessions.tsx, ProfileSettings.tsx
      Fault/AccessDenied.tsx, Suspended.tsx, IdentityUnavailable.tsx
    components/auth/
      AmbientBackground.tsx  # state-color living background (shared with the theme)
      ElysiaPresence.tsx     # reactive ambient geometry (shared with the theme)
```

The `AmbientBackground`, `ElysiaPresence`, sound, and design tokens are shared between the
Keycloakify theme and the SPA so both surfaces render identically.

## Build phases

1. **Session & gate.** Confirm the realm's confidential client + reverse-proxy topology; build the
   cookie-session client, `useSession`, `RequireAuth`, `RequireInstance`; retire `keycloak-js`.
2. **Keycloakify foundation.** Stand up the theme project, share design tokens with the app, and
   prove one template end-to-end (identify → passkey).
3. **Full ceremony set.** All Surface A templates, including enrollment and recovery codes.
4. **Bootstrap & invites.** Claim Instance wizard, signed invite links, and Invite Acceptance.
5. **Account & fault states.** Security center, active sessions, profile; access-denied,
   suspended, identity-unavailable (replacing the silent anonymous render).
6. **Cinematic layer.** Shared ambient background, Elysia presence, motion, sound, voice, copy
   pass, and the full accessibility + i18n sweep across both surfaces.

## Implementation status (frontend)

Phase 1 and much of phases 4 to 6 (Surface B) are built in `frontend/`:

- **Session & gate.** A cookie session (`credentials: 'include'`), a `session` Redux slice,
  `resolveSession` (probes `/api/v1/me`), `useSession`, and the `RequireAuth` / `RequireInstance`
  guards. `keycloak-js` and the browser-side token wiring are removed; the SPA holds no token.
- **Surface B screens.** Splash, sign-in entry, signing-in, signed-out, claim-instance, onboarding
  (welcome + profile), invite acceptance / invalid invite, security center, active sessions,
  profile settings, and the access-denied / suspended / identity-unavailable fault states, all
  routed with a full-screen auth layout separate from the in-app `AppLayout` chrome.
- **Cinematic placeholders.** `AmbientBackground`, `ElysiaPresence`, and `AuthShell` drive the
  state-color grammar with pure CSS/SVG stand-ins (no art assets yet) and honor
  `prefers-reduced-motion`.
- **i18n.** `i18next` + `react-i18next` are wired with `common` and `auth` namespaces (en-US).

Not yet built, pending the backend BFF: **Surface A** (the Keycloakify ceremony theme) and the
richer `/api/v1/auth/*` endpoints (bootstrap, invites, sessions, context). Screens that need those
endpoints are wired to the contract and degrade gracefully until the backend lands.

## Implementation status (backend)

The core BFF passthrough is built (`backend/src/oidc.rs`, `session.rs`; migration
`0002_sessions.sql`), so the frontend's sign-in path now has a real backend:

- **Confidential OIDC client.** The realm's client is `elysium-api` (confidential); the public SPA
  client is retired. `GET /auth/login` redirects the browser into Keycloak's Authorization Code +
  PKCE (S256) flow; `GET /auth/callback` exchanges the code over a backchannel, validates the ID
  token (signature + issuer via the JWKS path, plus the `nonce`), and establishes a session.
- **Server-side sessions.** Keycloak tokens live in the `sessions` table; the browser holds only an
  opaque HttpOnly, Secure session cookie. `GET /api/v1/me` reads the cookie (the SPA's session
  probe: 200 authed, 401 anon). `POST /api/v1/auth/logout` deletes the session and ends the
  Keycloak session. `GET /auth/account` deep-links to Keycloak's account console.
- **SSE rides the same cookie.** `GET /api/v1/events` authenticates with the session cookie (no
  token in the URL) and enforces a per-user connection cap (see `docs/realtime.md`).

Still open on the backend: `bootstrap` / `invites` / `context` / `sessions` endpoints, refresh-token
rotation behind the cookie, and encryption-at-rest for stored tokens.

Same-origin: the SPA, its `/api` routes, and its `/auth` routes are served from one origin (the
Vite dev server proxies `/api` and `/auth` to the backend; production uses a reverse proxy). So
the session cookie is first-party (`SameSite=Lax; Secure; HttpOnly`), there is no CORS, and the
OIDC redirect lands on the app origin and is proxied to the backend's `/auth/callback`. The
backend derives its URLs so `redirect_uri` is the app origin (`backend/src/config.rs`).

## Resolved decisions

- **Credential custody: Keycloak.** Passkey public keys and TOTP secrets are stored and verified
  by Keycloak. Keycloak owns user security and auth fully; Elysium's API never becomes a WebAuthn
  Relying Party.
- **Invite delivery: copyable signed link.** Invitations are self-contained signed tokens,
  delivered however the admin chooses, with no SMTP dependency.

## Open items

- **Reverse-proxy topology (production).** In dev the SPA proxies `/api` and `/auth` to the
  backend, so the app is already one origin and the session cookie is first-party. Production
  still needs the gateway that also fronts Keycloak on that same origin (subpath `/auth` vs.
  subdomain) and the matching WebAuthn RP ID.
- **Keycloakify build integration.** Wiring the theme build into `frontend/` and CI so the theme
  ships with the realm.
- **Session cookie specifics.** Server-side session store, idle/absolute lifetimes, and refresh
  strategy behind the cookie.
</content>
