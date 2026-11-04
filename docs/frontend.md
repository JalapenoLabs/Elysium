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
| Tests           | Vitest with jsdom and Testing Library                          |

## Serving

In production the API serves the app. The published image's `frontend` stage runs `yarn build`, and the API answers
every path outside `/api` from that build: files under `assets/` are fingerprinted and cached for a year, a missing
one is a `404`, and every other path is `index.html`, revalidated on each load. The image build writes Brotli and gzip
copies beside every text asset, which are sent as they are; the few other files are compressed per request. See
`api/src/web_app.rs` and `docs/infrastructure.md`.

In development the Vite dev server serves it with hot reload, behind nginx, and `/api` goes to the API.

The production pages carry a strict Content Security Policy (`docs/security.md`): no inline scripts and no eval.
Code here keeps to it:

- No inline `<script>`. The one script that must run before the first paint is `public/theme.js`.
- Zod is configured `jitless` in `main.tsx`, before any schema parses. Otherwise it probes for `new Function` once,
  and the policy reports that probe as a violation even though Zod recovers from it.
- Images come from the API or are `blob:` or `data:` URLs. Anything from another origin needs the policy widened
  first.

## Layout

`src/layout/AppShell.tsx` is the root route's element and renders on every page:

- `Sidebar` sits on the left from `md` up: the brand, then the primary navigation from `src/layout/navigation.ts`.
  Add top-level areas to that list, not to the component. Below `md` it is hidden, and `MobileNavigation` puts a
  menu button at the start of the topbar that opens the same `SidebarNavigation` in a drawer from the left; following
  a link closes it.
- `Topbar` spans the content column. It has a search field and, on the right, the Elysia button, the live updates
  dot, the settings gear, and the account menu (`UserMenu`): My profile, Sign-in & security, then Sign out below a
  divider, each with an icon. Pressing `/` outside a text field focuses search. Search itself does nothing yet. The dot is
  green while the event stream is connected and amber while it reconnects.
- `ElysiaPanel` (`src/elysia/`) is on the right while it is open: her status, questions, work, and the
  conversation or her activity. `AppShell` places it: beside the page from `2xl` up, floating over the page's right
  edge below that (pages lay out by viewport breakpoints, so squeezing them beside a 26rem panel breaks their
  columns), and full width below `sm`. The panel fills whatever it is given. The Elysia button toggles it, and `startElysiaPanelSync` remembers it per browser.
  `useElysiaSounds` plays her tones as her light changes. See `docs/elysia.md`.
- The routed page renders in `main` through `<Outlet />`. By default `main` is a padded, scrolling column. A route
  whose `handle` is `{ layout: 'workspace' }` (typed as `RouteLayoutHandle` in `src/router.tsx`) gets the whole
  content area and manages its own scrolling instead; the Coding page does.

Every page works at 375px. The shell pads `main` and the topbar `px-4` below `md` and `px-10` from it, and page
headers keep their primary action on screen, dropping secondary buttons to icons with an `aria-label` where they
would not fit.

`AppShell` also mounts HeroUI's `RouterProvider`, wired to React Router's `navigate` and `useHref`. That makes any
HeroUI `Link` `href` a client-side navigation. It also mounts the toast region.

## Routes

Paths live in `UrlTree` in `src/urls.ts`, and the route table lives in `src/router.tsx`. `/` and unknown paths
redirect to Action items, the first page in the sidebar; there is no home page.

| Path                          | Page                     | Notes                                                    |
|-------------------------------|--------------------------|----------------------------------------------------------|
| `/action-items`               | `NextPage`               | Next, one item at a time; first in the sidebar           |
| `/action-items/inbox`         | `InboxPage`              | Inbox triage, one item at a time                         |
| `/action-items/all`           | `ActionItemListPage`     | Every item, filtered from the address                    |
| `/action-items/new`           | `CreateActionItemPage`   | Add an item; `?project=` and `?initiative=` preset it    |
| `/action-items/:itemId`       | `ActionItemPage`         | One item, edited in place, live or deleted               |
| `/action-items/initiatives`   | `InitiativesPage`        | Initiatives with their progress                          |
| `/action-items/initiatives/new` | `CreateInitiativePage` | Start an initiative; `?project=` presets it              |
| `/action-items/initiatives/:initiativeId` | `InitiativePage` | One initiative: progress, burnup, items, history  |
| `/action-items/changesets`    | `ChangesetsPage`         | Changesets, those waiting for review first               |
| `/action-items/changesets/:changesetId` | `ChangesetPage` | One changeset's review, outcome, and undo         |
| `/projects`                   | `ProjectsPage`           | Projects as a table or tiles, searched and sorted        |
| `/projects/new`               | `CreateProjectPage`      | Create a project, with its cover                         |
| `/projects/:projectId`        | `ProjectPage`            | One project, edited in place, with its sessions          |
| `/coding`                     | `CodingPage`             | Dockview workspace; `?item=` opens New session started from that item |
| `/coding/:sessionId`          | `CodingPage`             | Opens session number `sessionId`, then returns to `/coding` |
| `/studio`                     | `StudioPage`             | Studio items as tiles, filtered from the address         |
| `/studio/:itemId`             | `StudioItemPage`         | One item: the stage beside its conversation, live or deleted |
| `/runs`                       | `RunsPage`               | Workflow runs, newest first, a page at a time; members only |
| `/runs/:runId`                | `RunPage`                | One run: its steps, each opening to its log, and its history |
| `/settings`                   | `SettingsDirectoryPage`  | Stripe-style directory; reached from the topbar gear     |
| `/settings/profile`           | `ProfilePage`            | My profile: name and email, and the light, dark, or system theme |
| `/settings/personal-details`  | redirect                 | To `/settings/profile`                                   |
| `/settings/browsers`          | redirect                 | To `/settings/connectors`                                |
| `/settings/browsers/:browserSessionId` | `BrowserSignInRedirect` | To the connector that owns the browser, else the list |
| `/settings/llms`              | `ManageLlmsPage`         | List, activate or deactivate, and delete LLMs            |
| `/settings/llms/add-<type>`   | `AddLlmPage`             | One per provider, see below                              |
| `/settings/llms/:llmId/edit`  | `EditLlmPage`            | Edit a credential; its provider type is fixed            |
| `/settings/email`             | `ManageEmailPage`        | Mail server and domains, and every kind of mailbox       |
| `/settings/environment`       | `ManageEnvironmentPage`  | Environment variables every coding session receives     |
| `/settings/environment/new`   | `AddEnvironmentVariablePage` | Add an environment variable                          |
| `/settings/environment/:variableId/edit` | `EditEnvironmentVariablePage` | Edit an environment variable              |
| `/settings/connectors`        | `ConnectorsPage`         | Every connector, its status, and activity               |
| `/settings/connectors/new`    | `ConnectorPickerPage`    | A searchable grid of every kind, each opening its form; `?provides=` narrows it |
| `/settings/connectors/new/:kind` | `NewConnectorPage`    | One kind's form: sign in, what it reads, live checks    |
| `/settings/connectors/:connectorId` | `ConnectorPage`    | One connector: status, its browser to sign in on, content, and checks |
| `/settings/github`            | redirect                 | To `/settings/connectors`                               |
| `/settings/github/new`        | redirect                 | To `/settings/connectors/new`                           |
| `/settings/github/:credentialId/edit` | `EditGithubCredentialPage` | Edit a GitHub token                            |
| `/settings/jira`              | redirect                 | To `/settings/connectors`                               |
| `/settings/jira/new`          | redirect                 | To `/settings/connectors/new`                           |
| `/settings/jira/:credentialId/edit` | `EditJiraCredentialPage` | Edit a Jira site                                   |
| `/settings/jira/:credentialId/done-transitions` | `JiraDoneTransitionsPage` | A project's done status; `?project=` picks it |
| `/settings/meetings`          | `MeetingsPage`           | The person's meeting sources and recent meetings         |
| `/settings/learnings`         | `LearningsPage`          | Learnings, the deleted ones, and for admins the feed, as tabs in `?tab=` |
| `/settings/learnings/new`     | `NewLearningPage`        | Save a learning; a takeaway one already has revises it   |
| `/settings/learnings/:learningId` | `LearningPage`       | One learning, corrected in place, with its history and Undo |
| `/settings/learning-categories` | `LearningCategoriesPage` | Every learning category, the workspace's own reordered and removed from the row menu; admins |
| `/settings/learning-categories/new` | `NewLearningCategoryPage` | Add one of the workspace's own categories; admins |
| `/settings/learning-categories/:categoryKey/edit` | `EditLearningCategoryPage` | Rename or describe one; under `/edit`, since a key may be `new` |
| `/settings/storage`           | `ManageStoragePage`      | Storage locations Elysium saves files to                 |
| `/settings/storage/new`       | `AddStorageLocationPage` | Add a storage location                                   |
| `/settings/storage/:locationId/edit` | `EditStorageLocationPage` | Edit a storage location                         |

The settings directory groups entries into titled sections, each a responsive grid of `SettingsDirectoryItem`s: Personal
settings, Knowledge base (meetings; learnings, hidden from guests; learning categories, for admins alone; and the knowledge
base's own settings as they are built), and Workspace settings.
New settings pages add an entry to a section and a route under `/settings`, and render inside `SettingsPage`.

### Settings pages

Every settings subpage renders inside `SettingsPage` (`src/pages/Settings/SettingsPage.tsx`), which holds the saving
and keyboard rules every one of them follows, so a page only describes itself:

```tsx
<SettingsPage
  title={t('page.editTitle', { name })}
  // Optional, leading the title, such as a connector kind's logo.
  icon={<ConnectorLogo kind={kind} />}
  // Pages between Settings and this one, for the breadcrumbs and Esc. Omit at the top level.
  parents={[{ label: t('title'), href: UrlTree.settingsLlms }]}
  // Optional, shown left of Save, such as a list page's Add button.
  actions={<Button variant='outline'>...</Button>}
>
  <LlmForm />
</SettingsPage>
```

- **Header.** Breadcrumbs, then the title on the left and the page's actions and Save on the right.
- **One Save per page.** The form that owns the page's settings registers it, however deep it sits, with
  `useSettingsSave` (`src/pages/Settings/settingsSave.ts`), and puts the same id on its `<Form>`:

  ```tsx
  useSettingsSave({
    formId: LLM_FORM_ID,
    isDirty: form.formState.isDirty,
    isPending: form.formState.isSubmitting,
    // Optional: why Save is disabled, shown in its tooltip; and a label other than Save.
    blockedReason: null,
    label: t('actions.connect'),
  })
  ```

  Save submits that form (`type='submit' form={formId}`), so Save, Enter, and Ctrl+S share the form's own submit
  handler and validation. It is enabled while the form is dirty, not pending, and not blocked. Forms keep no save
  button or cancel button of their own; the breadcrumbs and Esc go back. A page with nothing to save, such as a list,
  registers nothing and shows no Save.
- **Floating Save.** While the header's Save is scrolled out of view and the form is dirty, `FloatingSaveCard` offers
  it in the bottom right. Dismissing it lasts until the header's Save is back in view.
- **Keyboard.** Esc goes to the last parent, or `/settings`, asking first when the form is dirty. It is left alone
  when a component already used it (`defaultPrevented`), inside a menu or list box, and while a modal is open, so a
  modal closes first. Ctrl+S (Cmd+S) never opens the browser's save dialog on a settings page, and presses Save when it
  can. The rules are `readSettingsShortcut` in `settingsShortcuts.ts`.
- **Feedback.** A save is confirmed with a toast alone. Nothing a save reports may move the page, so there are no
  success banners above the form.

## Accounts

The account pages sit outside the app shell: `/login`, `/signup`, `/recovery`, `/pending`, and `/auth/error`. They
render Ory Kratos's self-service flows through its JSON API at `/api/identity/`, using `@ory/client-fetch`
(`src/api/kratos.ts`, `src/api/kratosFlows.ts`). Passkeys go through `@simplewebauthn/browser`, and the email field
offers them in autofill. See `docs/auth.md`.

- **The gate.** `src/auth/AuthGate.tsx` wraps every workspace page. It loads `GET /api/v1/me` into `authSlice` and
  sends people where they must go first: sign-in, the second factor, `/pending`, the disabled notice, or Sign-in &
  security when the workspace requires an authenticator app.
  - **Refusals.** Any request's `401` or coded `403` lands in Redux through the ky hook in `src/api/index.ts`, and the
    gate routes on it the same way.
  - **Event stream.** The stream opens only once the person may use the workspace, and closes on sign-out.
- **The waiting page.** `/pending` asks `/me` every 5 seconds. It is the one page that polls: a pending person may not
  open the event stream.
- **Kratos settings flows.** My profile and Sign-in & security each run one through `useKratosSettingsFlow`
  (`src/hooks/`), named with the page a change needing a recent sign-in sends the person back to, from `/login` in
  refresh mode. A field's refusal shows on the field; Kratos's messages for the whole form become toasts, and the
  success it reports after every save is dropped for the page's own toast, so nothing moves the page.
- **My profile** (`/settings/profile`). First and last name side by side (`PersonNameFields`, also on sign-up), the
  email below them, saved to Kratos by the page's Save; then the theme, which applies as soon as it is picked.
- **Sign-in & security** (`/settings/security`). Tabs, the open one kept in `?tab=`: General, Passkeys, Lookup codes,
  and Sessions, where lists show their totals beside their names.
  - **General.** A Password card whose Change password opens `ChangePasswordModal`, which asks for the new password
    twice. A Two-factor authentication card showing whether it is on, the authenticator app by its nickname, and
    whether lookup codes are set up, with Add 2FA, or Change 2FA and Remove 2FA. Adding sets the app up in a modal
    (`AuthenticatorEnrollment`: an optional nickname, the QR code and key, and the code). Changing renames the app, or
    moves to a new one, which removes the old app first, since Kratos offers a new QR code only then, and sets the
    new one up in the same modal.
  - **Passkeys, Lookup codes, Sessions.** Each list with its action above it.
- **Users** (`/settings/users`). Admins only. Workspace access settings, then pending sign-ups pinned above everyone
  approved. Users flow through SWR, then `usersSlice`, then the `user.*` events. The directory entry is hidden from
  others and badges the pending count.
- **Updates** (`/settings/updates`). Admins only. The release running, auto-update's state with what it waits for, a
  warning when no updater is connected, Check now, Update now for a release that rolled back, and the update history.
  The status flows through SWR, then `updatesSlice`, then the `updates.status` event. The directory entry is hidden
  from others.
- **Backups** (`/settings/backups`). Admins only. The storage location the databases are backed up to (saved with the
  page's Save; Nowhere turns backups off), the last backup stored, the next one or that backups are off, how long
  they are kept, Back up now in the header, and every attempt. The status flows through SWR, then `backupsSlice`, then
  the `backups.status` event. The directory entry is hidden from others. See `docs/infrastructure.md`, Backups.
- **Password field.** `PasswordInput` is the one password field. It shows a zxcvbn strength meter
  (`@zxcvbn-ts/core`, with dictionaries loaded only where the field is) and a checklist: 12 or more characters, an
  uppercase letter, and a symbol. Forms stay disabled until every rule passes and the score is at least 3.
- **Names.** `UserName` and `useActorNames` turn a user id or a recorded actor into a name: `user:<id>` is the
  person, and "You" for oneself. A bare `user` is from before accounts. Resources show who created them with
  `createdBy`.
- **Kratos's messages.** These show in English as Kratos sends them. Only Elysium's own sign-up refusals (`4190001`
  and `4190002`) are translated, as are the API's coded refusals.

## Connecting MCP clients

- **OAuth pages.** Hydra sends someone connecting an MCP client to `/oauth/login` and `/oauth/consent`. Both sit inside
  the auth gate, in the account pages' layout, so a person who isn't signed in signs in first and comes back.
  - `/oauth/login` answers the challenge at once and follows the redirect.
  - `/oauth/consent` shows the client and the scopes it asks for, then Allow or Deny.
- **MCP** (`/settings/mcp`), in the settings frame with no Save:
  - The MCP server's URL, with the `claude mcp add` and `codex mcp add` commands.
  - Tabs: the person's connected clients, with Disconnect, and, for admins, every registered client, with Delete.
    Each tab carries its count, so both lists load with the page rather than when a tab opens.
  - This data is loaded with SWR and revalidated after a change; no event stream carries it.
  - The page is named for the protocol, so its address is too. `/settings/connected-apps`, its earlier address,
    redirects to it, so links and bookmarks to it still land.

See `docs/mcp.md`.

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

`src/gates/ReportIssueGate.tsx` holds the one report dialog, inside `AppShell` so its links navigate in the app. Any
surface that shows an error offers Report with `ReportIssueButton` or `useReportIssue()(subject)`; see
`docs/issue-reporting.md`.

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

Below the credentials, admins see Agent configuration (`AgentConfigurationSettings`): the https repository every
agent's Claude Code configuration is checked out from, saved to the workspace settings (see `docs/coding.md`).

Storage under `src/pages/Settings/Storage/` lists storage locations in a table (name, provider and region, zone or
bucket with directory, projects, limit, and a Studio default switch, `StudioDefaultSwitch`) whose row menu edits,
tests, or deletes one; delete confirms through `useConfirm`. Adding and editing are their own pages: `StorageLocationEditorLayout` holds the breadcrumbs and title,
and `StorageLocationForm` lays out the form beside a `StorageSetupChecklist` for the chosen provider.

The Provider picker offers three options (`STORAGE_OPTIONS`): Bunny Storage, Amazon S3, and Google Cloud Storage. The
last two are the API's `s3` kind with a `service`, split in the picker because each is set up in its own console. The
form keeps every option's fields and shows the chosen one's: zone and region for Bunny; bucket, AWS region, and access
key ID for S3. `toStorageProvider` builds the API's provider from them through a lookup per option. The secret's label
follows the provider's own word (Password, Secret access key, Secret), and it stays optional while editing until the
settings need a new one. Directory, projects (`ProjectScopePicker`, see `docs/storage.md`), and a limit in gigabytes
(a `NumberField` in the viewer's locale, beside a No limit switch) are shared. Labels, secret wording, and setup steps
are lookup tables in `storagePresentation.ts`. `StorageSetupChecklist` and `LlmSetupChecklist` both render
`src/components/SetupChecklist.tsx`.

Connectors under `src/pages/Settings/Connectors/` is the hub for every service Elysium reads: its list, the grid of
kinds to add, each kind's form, and each connector's page, which for a browser kind carries its browser's stream.
Every form and check list is drawn from the kinds the API defines, and each kind's logo is bundled in
`connectorLogos.ts`. See `docs/connectors.md`, In the web app. `/settings/github` and `/settings/jira` lead to it.

GitHub under `src/pages/Settings/Github/` is the edit page a GitHub account opens from the hub, built from
`GithubCredentialEditorLayout` and `GithubCredentialForm`. The form is a name, a token type picker, and the token,
beside a `GithubSetupChecklist` for the chosen kind. The token field is optional while editing and turns required as
soon as the kind changes, mirroring the API. `githubPresentation.ts` holds the labels, setup steps, and
`matchesGithubTokenKind`, which checks a token's prefix before the API spends a call on it. An expired token shows a
danger chip in place of its date. The default token wears a Default chip, and the row menu makes a token the default
or stops. `src/components/GithubTokenSelect.tsx` picks a token for a project (`ProjectGithubField` on the project
page, saved on change, with a notice when its token was deleted) and for a session (`CreateSessionModal`). Both offer
following the level above, no token, or a token by name. See `docs/github.md`.

Jira under `src/pages/Settings/Jira/` holds any number of Jira Cloud credentials, each one Atlassian account on one
site with the projects and boards Elysium may read. Nothing about a project or a board is ever typed in: every option
comes from what Jira said the token reaches.

- A site is added in the connector hub as a new Jira connector, its projects and boards picked from what Jira
  reports, and checked before it is saved; see `docs/connectors.md`.
- `EditJiraCredentialPage` renders `JiraCredentialForm`, which keeps the connection fields and loads today's options
  through SWR under `['jira-credential-projects', id]` and `['jira-credential-boards', id]`. An allowed id missing from
  those lists is one the token no longer reaches, named in a warning above the pickers once the lists have arrived.
  What the form sends follows the API's rule that it re-checks with Jira every allowlist it is given: an allowlist the
  user never touched is left out of the `PATCH` entirely, so a rename neither calls Jira nor disturbs a project the
  token has temporarily lost; one they did touch is sent holding only what Jira reports today; and typing another site
  URL disables the pickers and saves both allowlists open, since an id from the old site names nothing on the new one
  and the API refuses a move that brings neither. A `400` from the save sits above the whole form, because it can name
  the token, the site, or a project lost since the page opened.
- `JiraDoneTransitionsPage` picks one of the site's allowed projects (the same `['jira-credential-projects', id]`
  listing) into `?project=`, and `JiraDoneTransitionPanel` reads that project's done statuses and choice from the API
  each time it opens: the one chosen, the only one, or a warning to choose among several, with a select and Forget.
- `JiraConnectionFields` is the connection half both forms render. The token field is optional while editing and turns
  required as soon as the site or the account email changes, mirroring the API.
- `JiraScopeFields` is the allowlist half both forms render: an All projects switch over `JiraScopePicker`, and the
  same for boards. A board's id is a number over the API and a key in the picker, and this is the one place the two
  spellings meet. `jiraPresentation.ts` holds the site URL rule, `keepsStoredJiraToken`, `toJiraScopeSelection`, and
  the table's scope summary. `JiraSetupChecklist` renders `src/components/SetupChecklist.tsx`, as GitHub's and
  Storage's do. See `docs/jira.md`.

Environment variables under `src/pages/Settings/Environment/` follow GitHub's shape: a table (key in monospace, value,
description) whose row menu edits or deletes one, and add and edit as their own pages built from
`EnvironmentVariableEditorLayout` and `EnvironmentVariableForm`. A secret's value shows as a mask beside a Secret chip; a
visible one is truncated in monospace. The form is a key, a Secret switch, the value (a multi-line text area, or a
password field while Secret is on), and a description. `EnvironmentVisibilityNote` sits beside the form and above the
table, saying that the agent can read every value and that secret only hides it from Elysium and its session logs.
While editing a secret the value is optional and keeps the stored one, and it turns required when Secret is switched
off, mirroring the API. `environmentPresentation.ts` mirrors the API's refused-key rules (`getKeyRefusal`), so a key
Elysium would refuse is flagged before saving. See `docs/environment.md`.

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

### Learnings

`src/pages/Settings/Learnings/` is the minimal view of learnings (`docs/knowledge.md`, Learnings) until the Knowledge
page folds it in, in the settings frame:

- **The list** (`LearningsPage`, `LearningTable`) is uikit's `SmartTable`: the takeaway, opening the learning; its
  category; who first saved it and through which agent; and when it last changed. Both lists filter by category, any
  built-in one or the workspace's own, kept in `?category=`. Deleted learnings are a tab of their own, and admins get a
  third, the feed (`LearningFeed`): every change to every learning, newest first, read on with Show older. A guest is told the knowledge base is for members, as the API refuses them (`selectIsCompanyMember`).
- **New learning** (`NewLearningPage`, `LearningForm`) saves from the header's Save and opens the learning saved,
  saying whether it was new or revised one whose takeaway it named. What is typed is kept in session storage
  (`newLearningDraft.ts`) until it is saved; a blank form never overwrites it.
- **A learning's page** (`LearningPage`) corrects it with the same form, remounted whenever the learning changes so it
  always edits what the learning says now. Delete happens at once, with Undo in its toast; a deleted learning reads
  as it was, under a notice, with Restore. `LearningHistory` lists every change newest first, each with Undo.
- **Data.** Learnings flow through SWR, then `learningsSlice`, then `learning.changed`, which carries an id alone, so
  the stream loads the learning by id for the company's members and revalidates its history and the feed. The history
  and the feed are read from SWR alone, since no other view shares them, as an initiative's burnup is.
- **Categories.** `useLearningCategories` loads every category for the company's members (SWR, then
  `learningCategoriesSlice`, revalidated by `learningCategories.changed` and by `learning.changed`, which moves counts)
  and gives each its label: a built-in one from this app's translations, by `presentLearningCategory`, and the
  workspace's own as its admins wrote it. The form's select, its description line, the filter, and every chip read
  it; a key the list lacks reads as the key. A form holding a category that no longer exists moves to the first.
- `learningPresentation.ts` holds the built-in category labels and descriptions, `presentLearningCategory`, the
  history sentences, and `describeHistoryEntry`; `learningFormSchema.ts` mirrors the API's bounds, leaving which
  category keys exist to the API. Both are tested beside them.

### Learning categories

`src/pages/Settings/LearningCategories/` is where admins manage the workspace's own categories
(`docs/knowledge.md`, Categories), in the settings frame; anyone else is told it is for admins.

- **The list** (`LearningCategoriesPage`, `LearningCategoryTable`) shows every category in the order it is offered,
  unsorted, since the order is the point: its name over its key, its description, and its learnings. A built-in row
  says Built-in where the row menu would be; the workspace's own have a menu (`LearningCategoryRowActions`) to edit,
  move up or down, and remove.
- **Removing** (`RemoveLearningCategoryModal`) names the category, says in bold that it can't be undone, and holds its
  button for five seconds. When learnings are filed under it, it asks where they move, and the button waits for an
  answer too.
- **New and edit** are their own pages over `LearningCategoryForm`, saved from the header's Save. A new category's key
  follows its name (`keyFromLabel`) until the key is typed by hand, and is read-only once saved. What a new one holds
  is kept in session storage (`newLearningCategoryDraft.ts`) until it is saved. `learningCategoryFormSchema.ts`
  mirrors the API's bounds.

### Action items

`src/pages/ActionItems/`, `src/pages/Initiatives/`, and `src/pages/Changesets/` build the area `docs/action-items.md`
designs. Next, Inbox, All items, Initiatives, and Changesets are tabs under one heading (`ActionItemsLayout`, a parent route with an `Outlet`); the item,
initiative, and create pages stand on their own with breadcrumbs. The sidebar keeps its one Action items entry.

- **Next** (`NextPage`, `NextItemCard`) shows one item from Next with its badges, notes, projects and initiatives,
  comments, and history. Quick actions each have a key: Resolve `R`, Dismiss `D`, Snooze `S`, Wait on someone `W`,
  Comment `C` (focuses the composer), Start a coding session `G`, Skip `J`, and Open `O`. Keys go through uikit's `useHotkey`, wrapped by
  `useQuickActionHotkey` so they are ignored while typing, while a dialog, menu, or list box has focus, and with a
  modifier held. Acting takes the item out of Next, so the next one takes its place without a request. Skip sets an
  item aside for the visit only, and skipping the last one starts the round again (`nextRotation.ts`). While the inbox
  holds anything, `InboxLeadCard` leads the page, and while changesets wait for review, `ChangesetsLeadCard` leads it
  above that. The Inbox and Changesets tabs each carry a count of what waits in them. An empty Next says how many items wait on someone or are snoozed.
- **Next is computed in the browser.** `src/store/nextOrder.ts` mirrors the API's Next (which items, and
  `api/src/action_items/next.rs`'s order) and is tested against the same cases, the way `environmentPresentation.ts`
  mirrors the API's key rules; keep the two in step. `selectNextActionItems(state, now)` applies it to every live
  item. A snooze running out sends no event, so time-dependent views read `useNow`, which re-reads the clock every
  minute (`ACTION_ITEMS_CLOCK_TICK_MS`) and fetches nothing. The API's `next` route is not used by the frontend.
- **Inbox** (`InboxPage`, `InboxTriageCard`) takes the inbox oldest first: Accept `A`, Dismiss `D`, Open `O`, with the
  next few titles listed below.
- **All items** (`ActionItemListPage`) is `ActionItemTable` (uikit's `SmartTable`) under `ActionItemFiltersBar`:
  states (the inbox and open by default), project (or no project), initiative, waiting, snoozed, and a Deleted items
  switch that lists deleted items instead, each with Restore. Filters live in the address (`actionItemFilters.ts`),
  with only what differs from the defaults written, so a filtered list survives opening an item and coming back.
- **Item page** (`ActionItemPage`) edits the title and notes in place with `InlineEditableText`. `ActionItemActionBar`
  offers the state changes the state allows (`transitionsByState` mirrors the API's table), Snooze, Wait on someone,
  Start a coding session, and a menu to stop waiting or delete. `ActionItemSessions` lists the coding sessions started
  from the item, from the `codingSessions` slice, below its comments. `ActionItemDetailsPanel` saves priority, due date,
  projects, and initiatives as they change; projects and initiatives go through their per-id routes, one request per
  one joined or left (`membershipChanges.ts`). A deleted item is read-only under a banner with Restore.
- **Links** (`ActionItemLinks`, under the item's notes) lists the item's links from the `actionItemLinks` slice, the
  primary first, each with its provider, kind, key (opening it in the provider), and state. What the provider says now
  (title, status, assignee, priority, due date) comes from `useActionItemLinkRemotes`, which reads every link live
  through SWR and never enters Redux, since an item holds none of those fields; the recorded title and state show until
  it answers, and a link it could not read says why. A row menu makes a link primary or unlinks it. `PendingWrites`
  lists what a link still owes, with its tries, the provider's last answer, and Stop trying; a Jira close that failed
  links to the project's done status choice.
- **Link pickers** (`LinkTargetPicker`) choose one Jira issue, GitHub issue, or pull request: `LinkCredentialSelect`
  lists every Jira site and GitHub token in one select, keyed `provider:id`. Jira is searched by `JiraIssueSearch`,
  which builds JQL from the words or key typed (`buildIssueSearchJql` in `linkPresentation.ts`) once typing rests
  (`useDebouncedValue`, `LINK_SEARCH_DELAY_MS`). GitHub is a kind, a repository (`GithubRepositorySelect`, sharing the
  coding form's `['github-repositories', id]` listing), and that repository's open issues or pull requests narrowed by
  typing (`GithubIssueList`). Every list is a `ChoiceList`, which shows loading, the provider's refusal, emptiness, and
  a cut short answer. `LinkTargetModal` wraps the picker for Link on the item page and for From Jira or GitHub on New
  item, which creates the item from the pick with the page's preset project and initiative.
- **Snooze** (`SnoozeMenu`) offers later today (three hours), tomorrow morning, next Monday morning, or a day picked
  from a calendar; a day snooze wakes at 09:00 in the viewer's zone (`SNOOZE_WAKE_HOUR`). **Due and target dates** are
  picked as days and stored as the last millisecond of that day in the viewer's zone, so an item due today is overdue
  tomorrow (`actionItemDates.ts`).
- **Comments** (`ActionItemComments`) post with Ctrl or Cmd and Enter. Only the user's own comments offer edit and
  delete; an edited one says so.
- **History** (`HistoryTimeline`) lists entries newest first, each an actor and a sentence built by
  `historyPresentation.ts` from the entry's kind and data, with every field an edit changed. Unknown kinds and
  malformed data still read as something.
- **Initiatives** (`InitiativesPage`, `InitiativeTable`) list name, state, progress, target date, and projects, active
  ones by default, with a Deleted initiatives switch like items'. Progress is always resolved and total together
  (`InitiativeProgressSummary`), with a bar beside the counts, never a percentage.
- **Initiative page** (`InitiativePage`) edits name and description in place and state, target date, and projects in
  `InitiativeDetailsPanel`. `BurnupChart` draws total and resolved as steps from the API's burnup, with a legend,
  direct end labels, a crosshair and tooltip that snap to the nearest change (by pointer or arrow keys), and the same
  numbers in a table beneath; its geometry is `burnup.ts`. `InitiativeMembers` lists the items with a button to take
  one out, adds existing items from a search, and links to a new item started in the initiative.
  `InitiativeContainers` lists the epics, saved filters, milestones, and labels it follows from the `initiativeLinks`
  slice, each with when the watcher last read it or why it could not, and whether it held more than the watcher reads.
  `AddContainerModal` picks one through `ContainerTargetPicker`: a Jira epic by search, a saved filter from
  `JiraFilterList`, or a repository's milestone or label from `GithubContainerList`. `containerPresentation.ts` holds
  which kinds each provider has.
- **Changesets** (`ChangesetsPage`, `ChangesetListItem`) lists the changesets waiting for review, then the reviewed
  ones, each with its summary, proposer, when it was proposed, its decisions so far, and its state.
- **Changeset review** (`ChangesetPage`, `ChangesetOperationCard`) shows each operation as a sentence and, for an edit,
  each field from what it holds to what it would hold (after applying, from what the operation replaced), with its
  reason, quote, and source, and which earlier change it acts on. While the changeset is pending, each change has
  Approve and Reject, the page has Approve all and Reject all, and a change says which others rejecting it also
  rejects (`dependentsOf` mirrors the API's rule). Apply is enabled once every change is decided. After, each change
  shows its decision and outcome with the reason it failed or was skipped; an applied comment or resolve lists the
  provider writes it still owes from the item's links (`OperationProviderWrites`), with the provider's last answer.
  Undo confirms first, saying that a posted comment and a closed issue stay; afterwards each change says what undo
  kept. `changesetPresentation.ts` holds the sentences, labels, and rules, tested beside it, and
  `useChangesetActions` the writes. A history entry a changeset made links to its review (`HistoryTimeline`).
- **Project page** (`ProjectWork`) lists the project's inbox and open items and its initiatives, each with a New
  button that presets the project, and a link to every item of the project in All items.
- **Reply** (`src/replies/`). An item a message raised shows a reply box under its links (`ActionItemReply`, loading
  the record with `GET /timeline?actionItemId=`), and a project's timeline offers Reply on each message
  (`RecordReply`). Both appear only where the record's kind declares `replies` other than `never`
  (`useRecordReplies`), so no page names a provider. `ReplyComposer` picks where a chat reply goes (in the thread or
  the conversation), or for mail the person's own mailbox, pointing at Settings, Email when they have none; it
  continues the record's newest unsent draft from the `drafts` slice, and Send writes or edits that draft and sends it
  (`docs/connectors.md`, Sending). A send that failed shows its reason under the text it kept, with Report. What is
  typed is kept in session storage per record (`REPLY_DRAFT_STORAGE_PREFIX`) until it is sent or discarded.
  `draft.changed` reloads a draft by id, and drops one that is gone.
- **Start a coding session**, on the item page and in Next, goes to `/coding?item=<id>` (`getNewCodingSessionUrl`). The
  Coding page opens New session started from the item once Dockview is ready, then returns to `/coding`; see
  [Coding](#coding).

The shared `src/components/` pieces are `DayPicker` (a date without a time), `MultiPicker` (tags from a searchable
list), `OptionSelect` (a labelled select over a short list), and `EmptyNotice` (what a list shows instead of an empty
table).

### Coding

`src/pages/Coding/` is a Dockview workspace with a Sessions overview panel and one conversation panel per open
session. `docs/coding.md` describes the panels, the conversation rendering, and layout persistence. These details
matter when changing it:

- Dockview renders panels through portals, so panels share the page's React tree (Redux, i18n, router) but not its
  props. Page actions reach them through `CodingActionsContext`.
- `/coding/<number>` (`getCodingSessionUrl`) opens that session's conversation once Dockview and the sessions are both
  loaded, then replaces the address with `/coding`; a number no session has goes straight to `/coding`. The project
  page links sessions this way.
- The Sessions panel shows its sessions as a table or as tiles. `SessionsToolbar` heads both: a search field, the
  result count, and, on the right, a Table and Tiles switch (HeroUI's `ToggleButtonGroup`, as on the Projects page).
  Search matches the number, title, project, state, and last activity as shown, so the table's own search
  and toolbar are hidden. Both views read one `SessionSummary` per session, built by `describeSession` in
  `sessionPresentation.ts` (project name, activity state and its label, last activity in the viewer's zone), and
  `searchSessionSummaries` filters them once for either view. The table's columns render and sort from those
  summaries; last activity sorts by the instant rather than the words shown.
- `SessionTiles` is a grid that fits as many 16rem columns as the panel is wide, down to one at phone width. Each
  tile shows the number, title, state chip, project, and last activity, with `SessionRowActions` in its
  corner. The title's link stretches over the whole card, so clicking a tile opens the session as clicking the title
  in the table does; the menu sits above it. The panel is a Tailwind `@container`, so the toolbar and grid follow the
  panel's width, not the window's.
- The chosen view is saved per browser under `elysium.coding.sessions.view.v1` and defaults to Table. It is held the
  way the theme is: the `sessionsView` slice starts from the stored value (`readStoredSessionsView` in
  `sessionsView.ts`, which falls back to Table for anything unknown or unreadable), and `startSessionsViewSync`, run
  once from `main.tsx`, saves every `sessionsViewChanged` and follows a choice made in another tab. Search resets on
  each visit.
- Every table of sessions (Sessions here, and `CodingSessionsTable` on the project and item pages) opens with a thin
  `#` column: the
  session number, right aligned in tabular numerals, sorted as a number, sized by `SESSION_NUMBER_COLUMN_SIZING`.
  uikit holds columns to at least 160 pixels unless a column sets its own bounds, and left-aligns every header, so the
  `#` header carries `.numeric-column-header`, which `src/index.css` moves to the right edge. The tables' storage ids
  end in `.v2`, since uikit appends a column it has not seen to the end of a saved column order.
- `CreateSessionModal` is the dialog's frame. It holds `CreateSessionForm`, or, for a session started from an action
  item, `ActionItemSessionForm`, which loads the item first, since the form presets itself from it: the title, the
  item's projects (`getSessionProjectChoices` in `sessionItemProjects.ts` mirrors the API; an item in one project fixes
  it), and a required prompt. The form sends the prompt with the create, and the API queues it as the first turn.
- `CreateSessionForm` picks the session's repositories through `SessionRepositoriesField`. `GithubRepositoryPicker`
  is HeroUI's `Autocomplete` in multiple selection over the resolved token's repositories, loaded through SWR under
  `['github-repositories', credentialId]`; its value only counts the picks, since each pick is a row below it
  (`SessionRepositoryRow`) with an optional base branch (the default branch as placeholder) and a remove button.
  Add by URL keeps manual entry for other hosts and unlisted repositories, checked with the same URL rules. Rows are
  keyed by URL and matched to the list by repository identity, so a URL added by hand for a listed repository shows as
  picked. Rows stay when the token changes and their warnings follow it: a picked repository the token does not list,
  no token at all, a token that can read but not push, and, for a github.com URL the list lacks,
  `SessionGithubAccess` asking GitHub about that one repository. `SessionGithubTokenExpiry` sits under the token
  picker. With no token the picker is disabled with a link to Settings, GitHub. Duplicate and directory rules mirror
  the API (`sessionRepositories.ts`).
- `DockviewReact` needs a sized parent. The page is a flex column whose Dockview wrapper is `min-h-0 flex-1`,
  inside a workspace-layout `main`.

### Studio

`src/pages/Studio/` is the grid of Studio items and the item page, a stage beside the item's conversation.
`docs/studio.md` describes both, the 3D viewer, and Annotate.

- **Loading.** The grid's two lists and the item page each have a loader. An item's response fills four slices at
  once: the item, its assets, its feedback, and its sessions. Sessions join the shared `codingSessions` slice through
  `codingSessionsUpserted`, which never replaces the Coding page's.
- **Deletes.** `studioItem.deleted` carries only an id, so, as for action items, it revalidates the deleted list
  and the item's own page.
- **Reuse from Coding.** The conversation reuses `TimelineEventList` and `useFollowingScroll`, and the composer
  reuses `PromptComposer` and its `sendBlocker`, which disables Send with the reason in a tooltip.
- **Pure logic.** Grouping files by stem, the stage's default subject, layout modes and the stored layout, tile
  thumbnails, filters, location choices, and drawing geometry are pure modules with tests
  beside them.

### Runs

`src/pages/Runs/` shows the engine's workflow runs (`docs/engine.md`, Runs). Runs read what was ingested, so the
sidebar's Runs link and both pages are for members alone; a guest who opens one is sent to the start.

- **The list.** `RunsPage` is a `SmartTable` of every run read so far: the workflow and its source, its state, who
  started it, when, and how long it took. It reads the newest page of 50, and Show older runs reads each page after,
  merged into the `runs` slice.
- **A run.** `RunPage` shows the run's state, duration, and the reason it stopped, then its steps as GitHub Actions
  shows a job's: each row collapsed to its state icon, its name, the line it wrote or why it failed, and how long its
  last attempt took, counting up while it runs. A row opens to its log, numbered line by line, which is read only
  then. A failed step starts open. The run's history follows.
- **Actions.** Each row ends with the icon buttons its state allows (`stepActionsByState`): Retry on a failed step,
  Resume on a held one, and Skip on any not yet done. The run the API answers lands in Redux at once; a `409`, from
  a step that moved on meanwhile, says so and reloads the run.
- **Live.** `workflowRun.changed` reloads that run by id, once the list was read or the run is shown;
  `stepJob.changed` reloads a step's log, once it was opened. Workflows, steps, and sources are named in code, and
  the pages read those names as words (`nameFromIdentifier`) until workflows bring words of their own.

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
confirms through `useConfirm` and only explains while sessions remain. Below, `CodingSessionsTable` lists the
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
| `actionItems`    | Live and deleted items in separate halves, newest first                 |
| `actionItemComments` | Comments of the items viewed, oldest first                          |
| `actionItemHistory` | Item and initiative history entries, oldest first                    |
| `actionItemLinks` | Links of the items viewed, the primary first, then oldest first         |
| `changesets`     | Every changeset, newest first                                           |
| `runs`           | Runs read so far, newest first, the steps and history of the runs viewed, and the logs opened |
| `elysia`         | Elysia at a glance, her conversation oldest first, her turns newest first, and whether her panel is open |
| `initiativeLinks` | Containers of the initiatives viewed, oldest first                     |
| `initiatives`    | Live and deleted initiatives in separate halves, by name                |
| `llms`           | LLM credentials, sorted by priority                                     |
| `mailAccounts`   | Connected mailboxes, sorted by address                                  |
| `mailDomains`    | The mail server's domains, sorted by name                               |
| `mailServer`     | The mail server's state, replaced whole by every update                 |
| `notifications` | Toasts kept for the notification menu this session, newest first        |
| `projects`       | Projects, sorted by name                                                |
| `githubCredentials` | GitHub tokens, sorted by name                                        |
| `jiraCredentials` | Jira Cloud credentials, sorted by name                                 |
| `learnings`      | Live and deleted learnings in separate halves, last changed first       |
| `environmentVariables` | Environment variables, sorted by key                              |
| `storageLocations` | Storage locations, sorted by name                                     |
| `studioItems`    | Live and deleted Studio items in separate halves, newest first          |
| `studioAssets`   | Every version of the files of the Studio items viewed, newest first     |
| `studioFeedback` | Drawn prompts of the Studio items viewed, oldest first                  |
| `codingSessions` | Coding and Studio sessions with what each is doing, newest first        |
| `drafts`         | The person's drafts of the records viewed, newest first                 |
| `sessionEvents`  | Events for conversations that are open, merged by sequence              |
| `sessionsView`   | Whether the Coding page's Sessions panel shows a table or tiles         |
| `realtime`       | Event stream connection: `connecting`, `open`, or `reconnecting`        |
| `theme`          | Theme preference and what it resolves to                                |

Server collections use entity adapters. Redux is the source of truth components render from.

### Server data: SWR, then Redux, then the event stream

1. **SWR loads it once.** A component that shows server data calls a loader from `src/hooks/useServerData.ts`
   (`useLlmsLoader`, `useMailAccountsLoader`, `useMailServerLoader`, `useMailDomainsLoader`, `useProjectsLoader`,
   `useStorageLocationsLoader`, `useGithubCredentialsLoader`, `useJiraCredentialsLoader`,
   `useEnvironmentVariablesLoader`, `useLearningsLoader`, `useDeletedLearningsLoader`, `useLearningLoader`,
   `useCodingSessionsLoader`,
   `useSessionHistoryLoader`, `useActionItemsLoader`, `useDeletedActionItemsLoader`, `useActionItemLoader`,
   `useActionItemCommentsLoader`, `useActionItemHistoryLoader`, `useInitiativesLoader`, `useDeletedInitiativesLoader`,
   `useInitiativeLoader`, `useInitiativeHistoryLoader`, `useActionItemLinksLoader`, `useInitiativeLinksLoader`,
   `useChangesetsLoader`, `useChangesetLoader`, `useElysiaLoader`, `useElysiaMessagesLoader`, `useElysiaTurnsLoader`,
   `useStudioItemsLoader`, `useDeletedStudioItemsLoader`,
   `useStudioItemLoader`). SWR
   fetches the key once, deduplicates every component asking for it, and buffers the response so a remounted page
   renders at once while it revalidates.
2. **Redux holds it.** The loader puts the response in Redux (`llmsLoaded`, `sessionHistoryLoaded`, ...).
   Loaders return only `loading`, `loaded`, or `failed`; components select the data itself from Redux. Once
   `loaded`, a refetch never drops a view back to a spinner.
3. **The event stream keeps it current.** Every change the API announces is dispatched into Redux. The stream
   subscribes before the workspace reads anything (`AuthGate` waits for it to open), so a page load reads each key
   once. On a reconnection's `hello` and on `resync` the stream revalidates every mounted SWR key, closing the gap
   from while it was disconnected. See `docs/realtime.md`, Frontend.

Collection loaders dispatch from inside the fetcher, so only a fresh network response replaces a collection;
SWR's buffered copy predates the stream's updates and must not roll Redux back. History merges by sequence
instead of replacing, so its loader applies the buffered copy too.

SWR's own focus and reconnect revalidation is off (`src/main.tsx`), because the event stream covers both. SWR is
also the tool for one-off reads that no global view shares, such as data a form needs to open.

After a write, dispatch the response (for example `llmUpserted(response.llm)`) so this tab updates without waiting
for the event. The event that follows is idempotent.

`sessionEvents` keeps a conversation's events only while its panel is open, capped at 5000. Live events for other
sessions are dropped.

Action items and initiatives keep live and deleted records apart because each list answers one or the other: the
live load replaces only the live half, and deleted ones load only while a view asks for them. `actionItem.deleted`
and `initiative.deleted` carry only an id, so they drop the record from the live half and revalidate the deleted
list's key and the record's own page, each of which refetches only if a view holds it, so an open page shows the record
as deleted rather than missing. Items list only live initiatives, so a deleted initiative's page says its items return
once it is restored instead of listing them. An upsert files a record by its `deletedAt`, so a restore moves
it back. An initiative's burnup is the one view read from SWR rather than Redux (`useInitiativeProgress`): no event
carries it, so `initiative.upserted` revalidates its key, which refetches only while an initiative page shows it.

Workspace, mail server, and broker failures answer `502` with the upstream's message. `getUpstreamErrorMessage` in
`src/api/errors.ts` extracts it for toasts.

## Forms

- react-hook-form with a Zod schema. Schemas are built with `t` so validation messages arrive translated.
- Read field values with `useWatch`, not `form.watch`. The React Compiler lint rejects `watch`.
- HeroUI v3 field `onChange` handlers receive the value, not a DOM event.
- A `Switch`'s clickable part is `Switch.Content`, so it wraps the `Switch.Control` and the `Label` together. A
  `Description` sits beside `Switch.Content`, inside the `Switch`. A control outside `Switch.Content` looks right but
  does nothing when clicked; `src/bugs/switchClickTarget.test.tsx` fails the build on one.
- Server conflicts map back to fields. For example, a `409` on create or edit sets an error on `name`.
- Secret tokens are write-only. The edit form leaves the field blank and sends a token only when one is typed.

## Overlays

A modal or alert dialog opened from code, not from a trigger child, skips the `Modal` or `AlertDialog` root. It
passes `isOpen` and `onOpenChange` to `Modal.Backdrop` or `AlertDialog.Backdrop`, fed by `useOverlayState`. The
root is a trigger wrapper, and rendering it without a pressable child makes React Aria warn.

## Toasts

Every toast goes through `toast` in `src/toast/toast.ts`, never HeroUI's: ESLint refuses importing `toast` from
`@heroui/react` anywhere else. It keeps HeroUI's shape (`toast`, `toast.success`, `toast.warning`, `toast.danger`,
`toast.close`) with string messages, and adds:

- **Timing.** Warnings and errors stay until dismissed. Everything else stays 1 second plus 300ms per word of its
  message, description, and action, and at least 6 seconds when it has an action. The policy is `toastTimeoutMs` in
  `src/toast/toastPolicy.ts`, with its numbers in `src/constants.ts`.
- **Action.** `action: { label, onPress }` puts one button on the toast, such as Undo; pressing it closes the toast.
- **Work in progress.** `toast.loading(message)` shows a spinner until `toast.close` is called with its id.
- **Kept for the notification menu.** Every toast but a success and a loading one is recorded in
  `notificationsSlice` (title, description, variant, and when), for the session only. A success confirms what the
  person just did, so it is left out; `keepInNotifications` overrides either way.
- **Slow changes.** The API client's `fetch` (`src/api/slowRequestNotice.ts`) shows "Your request is taking longer
  than usual... Hang tight." while any change (every method but `GET` and `HEAD`) has run for 5 seconds, under the
  10 second request timeout, and closes it when the last one returns. Reads are left out, since many run unasked.
- **No banners for outcomes.** What a submit reports (saved, refused, failed) is a toast, never an alert inserted
  above or below the form, so nothing moves the page. Alerts are for standing state that is there when the page
  opens, such as a vault that failed to start.

## Theming

`src/index.css` imports Tailwind, then `@heroui/styles`, in that order. The file also defines the shared layout
classes `relaxed`, `compact`, `level`, `level-left`, `level-right`, and `container`. Muted text uses opacity, not
gray colors, so the theme controls the base color.

### Light, dark, and system

Users choose Light, Dark, or System under Settings, My profile. The choice is saved per browser in
localStorage under `elysium.theme`, and defaults to System.

- The `theme` slice holds the preference and its resolved theme. `themeChanged(preference)` resolves System
  against `prefers-color-scheme` while preparing the action, so the reducer stays pure.
- `src/theme/themePreference.ts` has the storage and DOM halves: read and save the stored value, resolve, and apply
  the result to `<html>` as the `light` or `dark` class plus `data-theme` and `color-scheme`.
- `src/theme/startThemeSync.ts` connects them at startup. A listener saves and applies every `themeChanged`. The OS
  color scheme and other tabs' storage events dispatch `themeChanged` again.
- `useThemePreference()` reads the preference from Redux and dispatches changes.
- `public/theme.js`, loaded by `index.html` without `defer`, applies the saved theme before the CSS loads, so dark
  mode never flashes light on load. It is a file rather than an inline script because the page's Content Security
  Policy forbids inline scripts. It duplicates the resolve-and-apply logic in a few lines; keep the two in sync.
- The theme cards use HeroUI's `RadioGroup` with the preview artwork uikit exports. uikit's own `ThemeSelector`
  hardcodes its green brand color for the selected card.

### Palette: Matter

The app's colors come from the Matter VS Code theme (`tobiastimm.matter`). `src/theme/matter.css` sets HeroUI's
base variables for each theme. HeroUI derives the hover, soft, and secondary shades from them.

- **Dark** follows Matter's workbench colors: page `#14191f`, sidebar and panels `#0c0e13`, overlays `#21252b`,
  text `#e6e6e6`, muted `#7a9bc2`, and accent `#267fb5`. Status colors are Matter's terminal green `#95cc5e`, yellow `#ccb85e`, and red `#ff7583`.
- **Light** has no Matter original. It carries the same blue accent and slate hues onto white, with the status
  colors deepened for contrast.
- **Adjustments for legibility.** Link text uses a brighter blue in dark mode and a darker one in light mode,
  because `#267fb5` alone falls short of body-text contrast. Borders sit a step lighter than Matter's own, which
  vanish against the page.

### Fields

Fields are filled, in the manner of HeroUI v2's default inputs, rather than v3's bordered ones. Every field-like
control (inputs, text areas, selects, autocompletes, number and date fields, search fields) is borderless with a
soft fill (`#1c222b` dark, `#eef2f6` light) that shifts on hover, a 12px radius, a 40px height, and no shadow. Focus
shows HeroUI's 2px accent ring. Unchecked checkboxes keep a faint outline, since they share the field fill. All of
it is theme variables and a few class rules at the bottom of `src/theme/matter.css`, so components need no classes
of their own.

Style text links with `text-link`, not `text-accent`. The accent is for fills, focus rings, and selection.

`--sidebar` (`bg-sidebar`) is Elysium's own token for app chrome, registered with Tailwind through `@theme inline`.
`--chart-scope` and `--chart-resolved` (`bg-chart-scope`, `stroke-chart-resolved`, ...) are the burnup's two series.
They were checked as a pair against each theme's page for lightness, chroma, color-blind separation, and contrast;
dark mode steps them toward the middle of the lightness band, since Matter's accent and green sit outside it. Change
them together and check them again.

The same file points uikit's `--jala-table-*` variables at the palette, including the dark table colors.

`src/theme/dockview.css` defines `dockview-theme-elysium`, which points Dockview's `--dv-*` variables at the same
tokens.

## Translations

Every user-facing string goes through i18next. `src/i18n.ts` bundles the locale files and initializes before the
first render.

- `en-US` is the source locale and the only one shipped today.
- Namespaces are one file each under `src/locales/en-US/`: `common`, `navigation`, `settings`, `llms`,
  `email`, `storage`, `github`, `jira`, `environment`, `learnings`, `projects`, `coding`,
  `actionItems`, `initiatives`, `changesets`, `studio`, `elysia`, `runs`.
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

`yarn typecheck`, `yarn lint`, `yarn test`, and `yarn build` must all pass.

Tests are Vitest, beside the file they test (`nextOrder.ts` and `nextOrder.test.ts`), configured in
`vite.config.ts`. They run in jsdom; `src/testSetup.ts` loads the en-US translations, stands in for media queries, and
unmounts rendered components after each test. `src/testFixtures.ts` builds records as the API sends them. Slices and
selectors are tested against a fresh store from `createAppStore()`. A test that plays a whole page load calls
`vi.resetModules()` for fresh singletons; SWR is inlined for tests (`vite.config.ts`) so its cache resets with them.
Pure helpers, slices, selectors, and small
components are tested; pages are checked in the running app.

## Roadmap

- Additional locales, once the product adopts them.
- Tests for the older areas' pure logic, such as the timeline merge in `sessionEventsSlice`.
- Search results behind the topbar field.
- The theme preference moving from each browser to the person's profile.
