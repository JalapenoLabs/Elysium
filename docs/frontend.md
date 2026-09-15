# Frontend

Vite 8, React 19, and TypeScript in `frontend/`, managed with Yarn 4.17.0 (`packageManager` pinned). The UI uses
HeroUI v3 on Tailwind CSS v4 and is modeled on Stripe's dashboard.

## Stack

| Concern         | Choice                                                         |
|-----------------|----------------------------------------------------------------|
| Components      | HeroUI v3 (`@heroui/react`, built on React Aria Components)    |
| Org components  | `@jalapenolabs/uikit` (tables via `SmartTable`)                |
| Linting         | `@jalapenolabs/cli/eslint`, the org-wide ESLint 9 ruleset      |
| Styling         | Tailwind CSS v4 via `@tailwindcss/vite`                        |
| Routing         | React Router 8, data router (`createBrowserRouter`)            |
| State           | Redux Toolkit with react-redux                                 |
| Live updates    | One `EventSource` on `/api/v1/events`, see `docs/realtime.md`  |
| HTTP            | Ky                                                             |
| Docking layout  | Dockview (`dockview-react`) on the Coding page                 |
| Forms           | react-hook-form with Zod schemas                               |
| Dates           | `@internationalized/date` for input, `Intl` for display        |
| Icons           | `react-icons/lu` (Lucide)                                      |
| Translations    | i18next with react-i18next                                     |

## Layout

`src/layout/AppShell.tsx` is the root route's element and renders on every page:

- `Sidebar` sits on the left: the brand, then the primary navigation from `src/layout/navigation.ts`. Add
  top-level areas to that list, not to the component.
- `Topbar` spans the content column. It has a search field and, on the right, the live updates dot and the
  settings gear. Pressing `/` outside a text field focuses search. Search itself does nothing yet. The dot is
  green while the event stream is connected and amber while it reconnects.
- The routed page renders in `main` through `<Outlet />`. By default `main` is a padded, scrolling column. A route
  whose `handle` is `{ layout: 'workspace' }` (typed as `RouteLayoutHandle` in `src/router.tsx`) gets the whole
  content area and manages its own scrolling instead; the Coding page does.

`AppShell` also mounts HeroUI's `RouterProvider`, wired to React Router's `navigate` and `useHref`. That makes any
HeroUI `Link` `href` a client-side navigation. It also mounts the toast region.

## Routes

Paths live in `UrlTree` in `src/urls.ts`, and the route table lives in `src/router.tsx`. Unknown paths redirect to
the home page.

| Path                          | Page                     | Notes                                                    |
|-------------------------------|--------------------------|----------------------------------------------------------|
| `/action-items`               | `ActionItemsPage`        | Placeholder; first in the sidebar                        |
| `/studio`                     | `StudioPage`             | Placeholder                                              |
| `/`                           | `HomePage`               | Placeholder                                              |
| `/projects`                   | `ProjectsPage`           | Projects as a table or tiles, searched and sorted        |
| `/projects/new`               | `CreateProjectPage`      | Create a project, with its cover                         |
| `/projects/:projectId`        | `ProjectPage`            | One project, edited in place, with its sessions          |
| `/coding`                     | `CodingPage`             | Dockview workspace; `?session=<id>` opens that session    |
| `/settings`                   | `SettingsDirectoryPage`  | Stripe-style directory; reached from the topbar gear     |
| `/settings/personal-details`  | `PersonalDetailsPage`    | Appearance: light, dark, or system theme                 |
| `/settings/llms`              | `ManageLlmsPage`         | List, activate or deactivate, and delete LLMs            |
| `/settings/llms/add-<type>`   | `AddLlmPage`             | One per provider, see below                              |
| `/settings/llms/:llmId/edit`  | `EditLlmPage`            | Edit a credential; its provider type is fixed            |
| `/settings/satellites`        | `ManageSatellitesPage`   | Register satellites, see their status, test connections  |
| `/settings/email`             | `ManageEmailPage`        | Mail server and domains, and every kind of mailbox       |

The settings directory groups entries into titled sections, each a responsive grid of `SettingsDirectoryItem`s.
New settings pages add an entry to a section and a route under `/settings`.

## Pages and components

### Confirm and prompt gates

`src/gates/ConfirmGate.tsx` and `src/gates/PromptGate.tsx` wrap the router in `main.tsx`, each holding one dialog for
the whole app. `useConfirm()` and `usePrompt()` return a function that opens it with options, so a page describes
the question and what happens next instead of owning dialog state:

- `confirm({ title, message, tone, confirmText, canConfirm, onConfirm })`. `tone: 'danger'` shows a danger icon
  and button. `canConfirm: false` turns the dialog into an explanation with only Close, for an action that is
  blocked.
- `prompt({ title, label, defaultValue, description, isOptional, maxLength, isMultiline, onSubmit })` asks for one
  value and passes it trimmed.

Both keep the dialog open, the button pending, while the callback runs. It closes when the callback resolves and
stays open when it throws, so a callback reports its own failure (a toast) and rethrows to let the user retry.

Each exported component has its own file. Pages live under `src/pages/<Area>/`. Pure helpers, lookup tables, and
schemas sit in non-component files beside the page, such as `llmPresentation.ts` and `llmFormSchema.ts`, so React
Fast Refresh keeps working.

Manage LLMs is split into these files:

- `ManageLlmsPage` owns data loading and confirms deletes through `useConfirm`. Its Add LLM menu lists the provider
  types.
- `LlmTable` renders the credentials with uikit's `SmartTable`: search, result count, sorting, and column
  controls. Column widths and order persist in localStorage.
- `LlmRowActions` is the per-row menu to edit, activate or deactivate, and delete.
- `AddLlmPage` and `EditLlmPage` are full pages, not modals. Each provider type has its own add page:
  `add-claude-api-key`, `add-claude-code-oauth`, `add-openai-api-key`, and `add-codex-oauth` (`chatgpt-oauth`),
  mapped in `llmProviders.ts`.
- `LlmEditorLayout` puts `LlmForm` on the left and `LlmSetupChecklist` on the right: the steps for getting that
  type's token as checkboxes, with commands and links. Commands, paths, and URLs are data in `llmProviders.ts`
  and never translated. Checked steps are not saved.
- `LlmForm` handles create and edit. The token field has a reveal toggle. Expiry is a date, not a time: HeroUI's
  `DatePicker` (typed segments plus a calendar) at day granularity, saved as midnight at the start of that day in
  the viewer's zone. One year from now picks the date 365 days after today.

Manage satellites follows the same split under `src/pages/Settings/Satellites/`. Its table shows each satellite's
live status (online, unreachable with the reason on hover, checking, or inactive), version, and thread load, and
its row menu adds Test connection.

Email under `src/pages/Settings/Email/` has two sections.

`MailServerSection` follows the mail server's state from Redux. With no server it offers `CreateMailServerModal`
(first domain, and a hostname that follows it as `mail.<domain>` until edited); while it is created it shows the
current step from `mailServer.updated` events; once ready it lists domains in `MailDomainTable`, with
`AddMailDomainModal`, a remove confirmation (which only explains while the domain has mailboxes), and
`MailDomainDnsModal`, which checks the domain's records through SWR each time it opens and lists them with copy
buttons (`DnsRecordRow`).

The mailboxes section shows `MailAccountTable`. `MailboxSourceActions` offers Connect Gmail, Connect Outlook, and
Create mailbox, disabling each with the reason: no broker (from `GET /api/v1/mail/capabilities`, read through SWR),
no ready mail server, or no domain. `CreateMailboxModal` picks one of the server's domains. The Connect buttons are
plain anchors to the API's
OAuth start route, because the answer is a redirect to the broker; a client-side route change would not follow it.
`useOAuthOutcomeToast` announces the `mailConnected` or `mailError` the callback returns with, then strips it from
the URL. The row menu tests the connection, sends a test message, changes the sender name (through `usePrompt`),
toggles active, and disconnects; the disconnect confirmation says whether mail is deleted (self-hosted) or only
forgotten (OAuth).

### Coding

`src/pages/Coding/` is a Dockview workspace with a Sessions overview panel and one conversation panel per open
session. `docs/coding.md` describes the panels, the conversation rendering, and layout persistence. Two details
matter when changing it:

- Dockview renders panels through portals, so panels share the page's React tree (Redux, i18n, router) but not its
  props. Page actions reach them through `CodingActionsContext`.
- `?session=<id>` opens that session's conversation once Dockview and the session are both loaded, then removes the
  parameter. The project page links sessions this way.
- `DockviewReact` needs a sized parent. The page is a flex column whose Dockview wrapper is `min-h-0 flex-1`,
  inside a workspace-layout `main`.

### Projects page

`src/pages/Projects/ProjectsPage.tsx` lists projects in a table view (uikit's `SmartTable`) or a tiles view (HeroUI
cards). One toolbar drives both: search matches name and description, and sort is by name, session count, or last
update, in either direction. `projectListing.ts` does the filtering and sorting once, so switching views keeps
the same results in the same order. The table's own search and toolbar are hidden, and its column headers sort
through the same state as the toolbar. The chosen view is remembered per browser under
`elysium.projects.view.v1`; search and sort reset on each visit. The listing only opens projects: clicking a row
(uikit's row highlight, which never shows as highlighted) or a tile, itself a link, goes to `/projects/:projectId`.
The cover thumbnail opts out of the row click with `data-no-row-highlight`, so previewing it does not navigate.

`ProjectPage` edits the project in place. `ProjectBanner` runs the cover across the page below the name and
description: double-clicking it, or its Change cover button, picks an image and uploads it at once; without a cover
it is a single Add a cover image button. Beside Change cover, a Fit and Fill switch saves the project's `coverFit`,
which every rendering of the cover follows. Both controls show on hover or focus. The name and description are `InlineEditableText`, saved as soon as they are changed. The
Actions menu on the right (`ProjectActions`) holds what cannot be done in place: Remove cover, and Delete, which
confirms through `useConfirm` and only explains while sessions remain. Below, `ProjectSessionsTable` lists the
project's coding sessions; a row or title opens the session on the Coding page.

`src/components/InlineEditableText.tsx` shows text that becomes its own editor when clicked, in the same typography.
Enter saves a single line, Ctrl or Cmd with Enter a multi-line one, and clicking away saves too; Escape cancels. Its
`onSave` receives the trimmed value only when it changed, and throwing keeps the editor open on what was typed.

Each tile opens with `ProjectCover`, a 16:9 frame that follows the project's `coverFit`. With `fill` the cover covers
the frame, cropped to it, which suits photos. With `fit`, the default, it is contained, never cropped or stretched: a
square logo sits centered at full height, a wide banner centered at full width, and a blurred, enlarged copy of the
same image fills the space around it. A project without a cover shows its initial. The table view's first column shows the
same frame as a thumbnail, wrapped in `ImagePreview`.

`src/components/ImagePreview.tsx` makes any small image viewable larger: resting the pointer on it for two seconds
(`IMAGE_PREVIEW_HOVER_DELAY_MS`) shows a larger copy in a tooltip, and clicking it, or pressing Enter or Space while
it has focus, opens it fullscreen over a blurred backdrop. It wraps whatever thumbnail it is given, so the caller keeps
control of how the small image looks.

New project opens `/projects/new` (`CreateProjectPage`), which renders `ProjectForm` with the cover beside the other
fields. `ProjectCoverField` checks the type and the 10 MB limit before accepting a file (`projectCover.ts`, shared
with the banner) and previews it in the same frame. The form saves the project first and then uploads the cover,
since a new project has no id before it is saved; a failed cover leaves the saved project in place and says so. Cover
URLs come from `getProjectCoverUrl`, which adds `coverUpdatedAt` so a new cover is never served from cache.

## Jalapeno Labs packages

Both org packages install from GitHub, pinned to a commit in `package.json`. uikit is not on the npm registry. Yarn
only fetches git dependencies from approved repositories, so `.yarnrc.yml` approves
`https://github.com/JalapenoLabs/*`. The frontend image installs `git` for the same reason. To upgrade, change the
`#commit=` hash and run `yarn install`.

### `@jalapenolabs/uikit`

A React component kit. It styles itself with one scoped stylesheet (`jala-table-*` classes) themed through CSS
variables, so it does not depend on Tailwind or HeroUI. `src/index.css` points the table's variables at the
HeroUI theme so accents match.

- **`SmartTable`** takes `managedColumns` built with `createManagedColumns`. Each column needs a label, a search
  value (what the user sees, not raw data), and a `cell` renderer.
- **Labels.** Pass `useSmartTableLabels()` as `labels`. The kit ships English strings, and that hook supplies ours
  from i18next.
- **Row actions.** Render them as a regular column, as `LlmTable` does. The kit's built-in actions tray appears
  only on hover, above the row.
- **Empty lists.** The kit has no empty-state slot, so render the empty message instead of the table.

### `@jalapenolabs/cli/eslint`

`eslint.config.ts` extends the shared config. It adds two local blocks:

- **License header.** Headers must read `// Copyright © <year> Jalapeno Labs`. The shared config spells it
  `JalapenoLabs`.
- **React rules.** The shared config at commit `e3f02f8` intends to apply `react/recommended` and
  `react-hooks/recommended` to TSX, but drops them. It passes `fixupConfigRules`' array result along as if it
  were a single config. The local block restores those rules, using the same plugin versions, until the shared
  package is fixed.

`yarn lint` fails on any warning, and `yarn lint:fix` applies autofixes.

## Data

`src/api/index.ts` exports one Ky instance with `prefix: '/api'`. Requests are origin-relative because nginx
serves the frontend and API from the same host. Each resource has a file in `src/api/routes/` with one function per
endpoint and request and response types that mirror the Rust structs.

### Redux

`src/store/index.ts` holds the one store. Use `useAppSelector` and `useAppDispatch` from `src/store/hooks.ts`.
Selectors return existing references; never build objects or strings inside one.

| Slice            | Holds                                                                   |
|------------------|-------------------------------------------------------------------------|
| `llms`           | LLM credentials, sorted by priority                                     |
| `mailAccounts`   | Connected mailboxes, sorted by address                                  |
| `mailDomains`    | The mail server's domains, sorted by name                               |
| `mailServer`     | The mail server's state, replaced whole by every update                 |
| `projects`       | Projects, sorted by name                                                |
| `satellites`     | Satellites with their latest status                                     |
| `codingSessions` | Coding sessions with their thread state, newest first                   |
| `sessionEvents`  | Events for conversations that are open, merged by sequence              |
| `realtime`       | Event stream connection: `connecting`, `open`, or `reconnecting`        |
| `theme`          | Theme preference and what it resolves to                                |

Server collections use entity adapters. Redux is the source of truth components render from.

### Server data: SWR, then Redux, then the event stream

1. **SWR loads it once.** A component that shows server data calls a loader from `src/hooks/useServerData.ts`
   (`useLlmsLoader`, `useMailAccountsLoader`, `useMailServerLoader`, `useMailDomainsLoader`, `useProjectsLoader`,
   `useSatellitesLoader`, `useCodingSessionsLoader`, `useSessionHistoryLoader`). SWR fetches the key once,
   deduplicates every component asking for it, and buffers the response so a remounted page renders at once while
   it revalidates.
2. **Redux holds it.** The loader puts the response in Redux (`llmsLoaded`, `sessionHistoryLoaded`, ...).
   Loaders return only `loading`, `loaded`, or `failed`; components select the data itself from Redux. Once
   `loaded`, a refetch never drops a view back to a spinner.
3. **The event stream keeps it current.** Every change the API announces is dispatched into Redux. On `hello` and
   `resync` the stream revalidates every mounted SWR key, closing the gap from while it was disconnected.

Collection loaders dispatch from inside the fetcher, so only a fresh network response replaces a collection;
SWR's buffered copy predates the stream's updates and must not roll Redux back. History merges by sequence
instead of replacing, so its loader applies the buffered copy too.

SWR's own focus and reconnect revalidation is off (`src/main.tsx`), because the event stream covers both. SWR is
also the tool for one-off reads that no global view shares, such as data a form needs to open.

After a write, dispatch the response (for example `llmUpserted(response.llm)`) so this tab updates without waiting
for the event. The event that follows is idempotent.

`sessionEvents` keeps a conversation's events only while its panel is open, capped at 5000. Live events for other
sessions are dropped.

Satellite, mail server, and broker failures answer `502` with the upstream's message. `getUpstreamErrorMessage` in
`src/api/errors.ts` extracts it for toasts.

## Forms

- react-hook-form with a Zod schema. Schemas are built with `t` so validation messages arrive translated.
- Read field values with `useWatch`, not `form.watch`. The React Compiler lint rejects `watch`.
- HeroUI v3 field `onChange` handlers receive the value, not a DOM event.
- Server conflicts map back to fields. For example, a `409` on create or edit sets an error on `name`.
- Secret tokens are write-only. The edit form leaves the field blank and sends a token only when one is typed.

## Overlays

A modal or alert dialog opened from code, not from a trigger child, skips the `Modal` or `AlertDialog` root. It
passes `isOpen` and `onOpenChange` to `Modal.Backdrop` or `AlertDialog.Backdrop`, fed by `useOverlayState`. The
root is a trigger wrapper, and rendering it without a pressable child makes React Aria warn.

## Theming

`src/index.css` imports Tailwind, then `@heroui/styles`, in that order. The file also defines the shared layout
classes `relaxed`, `compact`, `level`, `level-left`, `level-right`, and `container`. Muted text uses opacity, not
gray colors, so the theme controls the base color.

### Light, dark, and system

Users choose Light, Dark, or System under Settings, Personal details. The choice is saved per browser in
localStorage under `elysium.theme`, and defaults to System.

- The `theme` slice holds the preference and its resolved theme. `themeChanged(preference)` resolves System
  against `prefers-color-scheme` while preparing the action, so the reducer stays pure.
- `src/theme/themePreference.ts` has the storage and DOM halves: read and save the stored value, resolve, and apply
  the result to `<html>` as the `light` or `dark` class plus `data-theme` and `color-scheme`.
- `src/theme/startThemeSync.ts` connects them at startup. A listener saves and applies every `themeChanged`. The OS
  color scheme and other tabs' storage events dispatch `themeChanged` again.
- `useThemePreference()` reads the preference from Redux and dispatches changes.
- An inline script in `index.html` applies the saved theme before the CSS loads, so dark mode never flashes
  light on load. It duplicates the resolve-and-apply logic in a few lines; keep the two in sync.
- The theme cards use HeroUI's `RadioGroup` with the preview artwork uikit exports. uikit's own `ThemeSelector`
  hardcodes its green brand color for the selected card.

### Palette: Matter

The app's colors come from the Matter VS Code theme (`tobiastimm.matter`). `src/theme/matter.css` sets HeroUI's
base variables for each theme. HeroUI derives the hover, soft, and secondary shades from them.

- **Dark** follows Matter's workbench colors: page `#14191f`, sidebar and panels `#0c0e13`, overlays `#21252b`,
  text `#e6e6e6`, muted `#7a9bc2`, and accent `#267fb5`. Inputs are `#0c0e13` with a `#343d64` border, as in
  Matter. Status colors are Matter's terminal green `#95cc5e`, yellow `#ccb85e`, and red `#ff7583`.
- **Light** has no Matter original. It carries the same blue accent and slate hues onto white, with the status
  colors deepened for contrast.
- **Adjustments for legibility.** Link text uses a brighter blue in dark mode and a darker one in light mode,
  because `#267fb5` alone falls short of body-text contrast. Borders sit a step lighter than Matter's own, which
  vanish against the page.

Style text links with `text-link`, not `text-accent`. The accent is for fills, focus rings, and selection.

`--sidebar` (`bg-sidebar`) is Elysium's own token for app chrome, registered with Tailwind through `@theme inline`.

The same file points uikit's `--jala-table-*` variables at the palette, including the dark table colors.

`src/theme/dockview.css` defines `dockview-theme-elysium`, which points Dockview's `--dv-*` variables at the same
tokens.

## Translations

Every user-facing string goes through i18next. `src/i18n.ts` bundles the locale files and initializes before the
first render.

- `en-US` is the source locale and the only one shipped today.
- Namespaces are one file each under `src/locales/en-US/`: `common`, `navigation`, `home`, `settings`, `llms`,
  `satellites`, `email`, `projects`, `coding`, `studio`, `actionItems`.
- `src/@types/i18next.d.ts` types every key, so a missing or misspelled key fails `yarn typecheck`.
- Enum values such as LLM types and statuses are translated through lookup tables typed with
  `satisfies Record<..., ParseKeys<'llms'>>`.
- Logs and `console.debug` messages stay in English.

## Time

The server sends and expects UTC ISO 8601 timestamps.

- Display: format with `Intl.DateTimeFormat` in the viewer's locale and time zone, as `LlmTable` does.
- Input: `DatePicker` holds a `CalendarDate` in the viewer's zone, read with `toCalendarDate(parseAbsoluteToLocal())`.
  It is submitted as `toZoned(date, getLocalTimeZone()).toAbsoluteString()`, the UTC instant of local midnight.

## Dev server

`yarn dev` binds on all interfaces on port 5173. Two environment variables adapt it:

| Variable                | Set by  | Effect                                                         |
|-------------------------|---------|----------------------------------------------------------------|
| `VITE_HMR_CLIENT_PORT`  | compose | HMR websocket connects through nginx's published port          |
| `VITE_API_PROXY_TARGET` | you     | Where `/api` goes outside compose (default `localhost:8080`)   |

Inside compose, nginx handles `/api` before Vite sees it. For a host dev server against the running stack, use
`VITE_API_PROXY_TARGET=http://localhost:4000 yarn dev`.

The compose frontend image installs `node_modules` at build time. After changing dependencies, run
`docker compose up -d --build frontend`.

## Checks

`yarn typecheck` and `yarn lint` must both pass.

## Roadmap

- Additional locales, once the product adopts them.
- A unit test setup (Vitest) for pure logic such as the timeline merge in `sessionEventsSlice`.
- Search results behind the topbar field.
- Contact and account fields on the Personal details page once user accounts exist, with the theme preference
  moving to the user profile.
