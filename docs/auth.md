# Authentication & Auth UI

This document is the scope and design decisions for Elysium's authentication
surface: how a user gets into the product, and every screen involved. The
authentication *model* (methods, ceremonies, sessions, org policy) is authoritative
in `auth-model.md`; this document owns the user-facing contract and UI.

## Principles

- **The API owns security fully.** The API stores and verifies every credential and
  brokers every ceremony (passkey today; OPAQUE passwords, TOTP, and magic links on
  the roadmap). The browser never leaves the app's origin to authenticate.
- **We own every pixel.** Every auth screen is Elysium's own React.
- **Tokens never live in the browser.** The browser holds only an HttpOnly, Secure,
  SameSite session cookie. Nothing auth-related is readable by JavaScript.
- **Passkey-first, not passkey-only.** A passkey (WebAuthn) is the flagship factor,
  and works with platform authenticators, hardware keys, and credential managers
  (Bitwarden, 1Password, ...). Passwords (OPAQUE) with a mandated TOTP second
  factor are the compatible path, governed by org policy.
- **Invite-only, with a first-run root.** A fresh instance is claimed by its first
  (root) user. Everyone after that arrives by a copyable signed invite link. There
  is no open public registration.
- **The first handshake feels like Elysium.** Auth is the first thing a user
  experiences, so it previews the product's soul: a living background that speaks
  the state-color language, an ambient Elysia presence, motion, and sound. It is
  cinematic, not a bare form.

## Architecture

One origin, one surface:

```
Browser ── one origin (app.example.com) ──┬─▶ /        SPA (the React app, including every auth screen)
                                          └─▶ /api/*   Elysium API (WebAuthn RP, sessions, app endpoints)
```

- **The SPA** renders the app and every credential screen, drives the native
  passkey prompt via `navigator.credentials`, holds only the session cookie, and
  reads auth state by probing `GET /api/v1/me`.
- **The API** is the WebAuthn Relying Party and the broker for every other method;
  it mints the session cookie on success and records every attempt in the login
  history.

## The frontend auth API contract

Implemented endpoints are listed in `auth-model.md`. The SPA is additionally built
against this planned contract (screens that need a missing endpoint degrade
gracefully until it lands):

| Endpoint | Purpose | Status |
|---|---|---|
| `GET /api/v1/auth/context` | Is this instance claimed yet? Drives first-run vs. sign-in. Public. | built |
| `POST /api/v1/auth/bootstrap/begin` + `/finish` | Claim an unclaimed instance: root user + first org + first passkey. | built |
| `POST /api/v1/auth/passkey/authenticate/begin` + `/finish` | Passkey sign-in. | built |
| `POST /api/v1/auth/logout` | End the session. | built |
| `POST /api/v1/auth/invites` | Mint an invite link (role/team/optional email). Authed. | built |
| `GET /api/v1/auth/invites/:token` | Preview an invite (org, team, role). Public; generic 404 when invalid. | built |
| `POST /api/v1/auth/invites/:token/accept/begin` + `/accept/finish` | Accept by enrolling a credential; provisions user + membership + recovery codes. | built |
| `POST /api/v1/auth/recovery/authenticate` | Sign in with a one-time recovery code. | built |
| `POST /api/v1/credentials/enroll/begin` + `/finish` | Authenticated add-credential (add passkey / set-or-change password). | built |
| `GET /api/v1/auth/credentials`, `GET /api/v1/auth/sessions`, `GET /api/v1/auth/history` | Security Center views: passkeys, active sessions, login history. | planned |

## Access model: invite-only, first-run root

- **First run.** On a brand-new instance there are zero users.
  `GET /api/v1/auth/context` reports the instance is unclaimed and the app routes to
  the **Claim Instance** wizard: it creates the root owner and first organization,
  enrolls the first passkey, then finishes the profile. Afterward the instance is
  claimed and the wizard never shows again.
- **Steady state.** Registration is invite-only. A member mints a **copyable invite
  link**; the invitee opens it, the app previews the org/team/role, then they accept
  by enrolling a credential (the same method-aware ceremony as first-run claim). No
  email delivery is required, which suits self-hosted and air-gapped installs.
- **No open signup.** There is no self-service public registration. A lost device
  is handled by recovery codes and, failing that, an admin re-issuing an invite.

### Invite links (built)

An invite is a row in `invites` (org, team, role, optional email, expiry); the API
returns a high-entropy link token once and stores only its SHA-256 hash, so the
copyable link is the secret (no SMTP, and a database read cannot mint working
links). `GET /api/v1/auth/invites/:token` previews it, answering a generic 404 for
a missing, expired, or already-accepted token. Acceptance is atomic and single-use:
`accept/begin` enrolls a credential (passkey or password + TOTP) exactly like claim
via the shared `enrollment` module, and `accept/finish` writes the user, their
membership, their recovery codes, and a session, and marks the invite consumed, in
one transaction. Any authenticated member may invite for now; finer-grained
authorization is a later hardening step.

### Recovery codes and re-enrollment (built)

Every enrollment (claim and invite acceptance) issues ten one-time recovery codes,
returned once for the user to save; only their hashes are stored. A lost-device
sign-in redeems one code at `POST /api/v1/auth/recovery/authenticate` (single-use,
atomic `UPDATE`; an unknown email and a wrong code are the same generic 401), which
establishes a session. The recovered user then enrolls a fresh credential via the
authenticated add-credential endpoints (`POST /api/v1/credentials/enroll/*`), which
also back the Security Center's "add a passkey / change password" actions. Listing
and removing credentials, and regenerating recovery codes, are the remaining
Security Center steps.

## Screen inventory (React Router routes)

The `State` column names the ambient background color from Elysium's state grammar.

| Screen | Route | Purpose | State |
|---|---|---|---|
| Splash / gate | `/` (pre-auth) | Living landing while the session resolves. First impression. | idle |
| Sign in (the Gate) | `/login` (`/sign-in` redirects here) | A full-bleed title screen, not a form: the wordmark is the hero and typing anywhere renders the email as giant glyphs (an invisible input owns focus). Enter runs method discovery; the passkey ceremony, the password stage, and the TOTP stage each play as a staged page turn. Local validation misses flash yellow; server failures show red. | idle → authenticating → needs-input |
| Signing in | transient | Post-ceremony handoff into the workspace. Cinematic, not a white flash. | authenticating → success |
| Claim instance | `/setup` | First-run root: owner + first org + first passkey. | idle → success |
| Onboarding | `/onboarding` | Welcome and profile (avatar, display name, status); wraps credential enrollment. | idle |
| Invite acceptance | `/invite/:token` | Verify the signed link, preview the org/team, enter enrollment. | idle → success |
| Invalid / expired invite | `/invite/:token` (fault) | Kind dead-end with a path to request a new invite. | error |
| Signed out | `/signed-out` | Confirmed sign-out, invite back in. | idle |
| Session expired | modal (any route) | Session ended; re-authenticate in place without losing work. | needs-input |
| Profile settings hub | `/settings` | Personal account settings, a simple flat sidebar; redirects to the profile section. | idle |
| Profile settings | `/settings/profile` | Given/family/display name, avatar URL, bio. | idle |
| Security center | `/settings/security` | Enrolled passkeys (added/last-used), password + authenticator status, add-passkey / add-authenticator, recent sign-ins. | idle |
| Active sessions | `/settings/sessions` | Live sessions with device, IP, last-active; revoke one or all others. | idle |
| Organization settings hub | `/organization` | Org-level settings, a grouped accordion sidebar (General, People); redirects to the general section. Reached from the taskbar org switcher. | idle |
| Org identity & branding | `/organization/general` | Name, logo, accent color, wordmark (owner/admin edit). | idle |
| Org members | `/organization/members` | Roster, role changes (incl. guest), suspend/reactivate, ownership transfer, removal (admin+). | idle |
| Org teams | `/organization/teams` | Teams and their members (admin+). | idle |
| Org roles & access | `/organization/roles` | Delegate admin capabilities to members (owner/admin). | idle |
| Org sign-in policy | `/organization/security` | Allowed methods, require-2FA, session lifetime (admin or manage_security). | idle |
| Org audit log | `/organization/audit` | Recent org activity (admin or view_audit). | idle |
| Suspended | `/suspended` | Where a member suspended in their active org lands (from `active_org_suspended`). | error |
| Two-factor gate | any protected route | Hard interstitial when a require-2FA org member has no authenticator; blocks the app until enrolled. | needs-input |
| Access denied | `/denied` | Authenticated but lacks the team permission. | error |
| Account suspended | `/suspended` | Brute-force lockout or admin suspension, with the right recovery CTA. | error |
| Identity unavailable | fallback | The API is unreachable. A branded, reassuring degraded state, not a silent anonymous render. | error |

## Enrollment & recovery

A lost device is the largest support burden of a passkey-first system. This is
designed in, not bolted on:

- **Enrollment always issues recovery codes.** Whenever a first passkey is created
  (root claim or invite acceptance), enrollment generates one-time recovery codes
  and requires the user to confirm they have saved them before continuing.
- **Recovery is a first-class sign-in path.** A lost-device sign-in consumes one
  recovery code, then immediately walks the user through enrolling a fresh passkey.
- **Admin re-invite is the last resort.** If recovery codes are also lost, an admin
  re-issues a signed invite, which resets enrollment.

## The cinematic layer

This is the "bring it to life" work, in scope for v1.

- **State-color grammar.** The ambient background shifts with the flow using
  Elysium's vocabulary: calm idle, **orange** while authenticating, **pink** when it
  needs your passkey tap, **red** on failure, a **green** flash on success. Auth is
  where users first learn this language.
- **Elysia presence.** A small ambient geometry (a nod to the workspace
  icosahedron) reacts to the flow: it pulses while thinking, leans in when it needs
  a tap, and settles once you are in.
- **Motion.** Framer Motion drives staged entrances, the passkey "tap" pulse, and a
  success transition that carries the user into the workspace rather than cutting.
- **Sound.** Short, cohesive cues: a soft chime on success, a distinct "your turn"
  cue when the passkey prompt appears (also an accessibility win), all behind a
  mute preference and `prefers-reduced-motion`.
- **Voice (opt-in).** Elysia can speak a brief "Welcome back" on success. Off by
  default.
- **Copy & tone.** Warm, confident, human. "Tap your passkey to continue," never
  "WebAuthn authentication required." Error copy helps and never blames.
- **Craft.** Full dark/light theming, complete keyboard navigation, screen-reader
  labels on every step, respectful reduced-motion, and fast perceived load via the
  splash.

Assets required for this layer are specified in `auth-art-briefs.md`.

## Internationalization

The SPA localizes with `i18next` under an `auth` namespace, `common` for shared
words. All locales the project ships are covered; `en-US` is the source.
Validation, empty states, and error copy are translated; logs and identifiers stay
English. See `i18n.md`.

## Frontend structure

```
frontend/
  src/
    auth/
      session.ts             # session state (probes GET /api/v1/me)
      useSession.ts          # hook exposing principal + status
      RequireAuth.tsx        # route guard; preserves intended destination
      RequireInstance.tsx    # routes to /setup when the instance is unclaimed
    api/routes/authRoutes.ts # bootstrapPasskey, signInWithPasskey, getMe, logout
    pages/
      Auth/Login.tsx         # the Gate: stage machine (identifier/passkey/password/TOTP)
      Auth/Login*Stage.tsx   # one file per stage
      Auth/SignedOut.tsx
      Setup/ClaimInstance.tsx
      Onboarding/Welcome.tsx, ProfileStep.tsx
      Invite/InviteAcceptance.tsx, InvalidInvite.tsx
      Settings/SecurityCenter.tsx, ActiveSessions.tsx, ProfileSettings.tsx
      Fault/AccessDenied.tsx, Suspended.tsx, IdentityUnavailable.tsx
    components/auth/
      AuthShell.tsx          # shared auth layout: ambient field + presence
      AmbientBackground.tsx  # state-color living background, pointer parallax
      ElysiaPresence.tsx     # reactive ambient geometry
      GhostInput.tsx         # type-anywhere invisible input, hero glyph rendering
      StageFlash.tsx         # yellow local-validation / red server-failure line
```

## Implementation status

Built today:

- **Session & gate.** A cookie session (`credentials: 'include'`), a `session`
  Redux slice, `resolveSession` (probes `/api/v1/me`), `useSession`, and the
  `RequireAuth` / `RequireInstance` guards. The SPA holds no token.
- **Passkey sign-in end to end.** The SPA drives `navigator.credentials` with
  `@simplewebauthn/browser` against the API's begin/finish endpoints; first-run
  claim enrolls the first passkey.
- **The Gate (`/login`).** The staged, full-bleed sign-in: identifier-first with
  type-anywhere input, the passkey ceremony, and password + TOTP stages that
  activate when method discovery advertises them. All flows run through the
  shared headless auth-method hooks (`src/auth/methods/`); the Gate only stages
  them. Discovery falls back to passkey-only if the methods endpoint fails.
- **Screens.** Splash, signing-in, signed-out, claim-instance,
  onboarding (welcome + profile), invite acceptance / invalid invite, security
  center, active sessions, profile settings, and the access-denied / suspended /
  identity-unavailable fault states, all routed with a full-screen auth layout
  separate from the in-app `AppLayout` chrome.
- **Cinematic placeholders.** `AmbientBackground`, `ElysiaPresence`, and
  `AuthShell` drive the state-color grammar with pure CSS/SVG stand-ins (no art
  assets yet) and honor `prefers-reduced-motion`.
- **i18n.** `i18next` + `react-i18next` with `common` and `auth` namespaces (en-US).

Remaining (tracks the roadmap in `auth-model.md`): removing individual passkeys and
regenerating recovery codes from the Security Center (listing credentials, sessions
with revoke, login history, add-passkey, and add-authenticator are live), and the
cinematic layer's real assets.

## Open items

- **Reverse-proxy topology (production).** In dev the SPA proxies `/api` to the
  backend, so the app is one origin and the session cookie is first-party.
  Production needs the operator's gateway to serve the SPA and API on one origin,
  with the WebAuthn RP ID matching that domain.
- **Session cookie specifics.** Idle lifetime and sliding renewal behind the
  cookie (absolute lifetime exists today; the org policy table already carries a
  configurable session lifetime for when renewal lands).
