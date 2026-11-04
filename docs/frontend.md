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
| Dates           | `@internationalized/date` for input, `src/format/` for display |
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

- `Sidebar` sits on the left from `md` up: the primary navigation from `src/layout/navigation.ts`, then the logo at
  its foot (`SidebarBrand`), full width, linking to `/`. Add top-level areas to that list, not to the component.
  Below `md` it is hidden, and `MobileNavigation` puts a menu button at the start of the topbar that opens the same
  `SidebarNavigation` in a drawer from the left, always expanded; following a link closes it.
  - **Links.** Large, padded, each with its icon. Hovering fills a link neutrally; the current page is accent-colored
    with a bar at its edge, so the two never look alike. They are React Aria `Link`s routed through the
    `RouterProvider` in `AppShell`, which gives the collapsed tooltips their trigger and reports hover only for a
    real mouse.
  - **Collapsing.** A small round chevron on the sidebar's edge, level with the middle of the topbar, collapses it to
    a 4rem column of icons and expands it again, each with a tooltip. It shows while the mouse is over the sidebar,
    while it has keyboard focus, and always on touch screens. Collapsed, the width clips the labels away (they stay
    the links' accessible names), each icon names itself in a tooltip to its right, and the logo shrinks to its mark.
    The choice is remembered per browser: `sidebarSlice`, stored under `elysium.sidebar.v1` by `startSidebarSync`,
    which also follows the choice made in another tab.
  - **Motion.** Collapsing, hovering, and the current page's bar animate in about 200ms. Every transition carries
    `motion-reduce:transition-none`, which HeroUI's variant applies under `prefers-reduced-motion` and under
    `data-reduce-motion`. All of it is classes, never injected styles.
- `Topbar` spans the content column. It has the global search (`GlobalSearch`, see Global search) and, on the right,
  the Elysia button, the live updates dot, the settings gear, the notification bell (`NotificationBell`, see Toasts),
  and the account menu (`UserMenu`), its avatar named in a tooltip: the person's name in small bold and their email in
  smaller italic, a divider, My account and Security as links, then Sign out below a divider, which asks in a dialog
  first; each item has an icon. The dot is green while the event stream is connected and amber while it reconnects.
- `ElysiaPanel` (`src/elysia/`) is on the right while it is open: her status, questions, work, and the
  conversation or her activity. `AppShell` places it: beside the page from `2xl` up, floating over the page's right
  edge below that (pages lay out by viewport breakpoints, so squeezing them beside a 26rem panel breaks their
  columns), and full width below `sm`. The panel fills whatever it is given. The Elysia button toggles it, and `startElysiaPanelSync` remembers it per browser.
  `useElysiaSounds` plays her tones as her light changes. See `docs/elysia.md`.
- `RefreshPrompt` sits in the content column's bottom left corner, clear of the toasts and the floating Save on the
  right, once Elysium runs a build other than the one the page loaded (`versionSlice`, fed by the event stream; see
  `docs/realtime.md`, Frontend): "A new version of Elysium is ready." with Refresh and a dismiss button. Dismissing it
  lasts until a later build arrives. Builds are named by `builtAt` from `/api/version`, since a build of `main` has no
  release and every image built from a new commit has a new time.
- The routed page renders in `main` through `<Outlet />`. By default `main` is a padded, scrolling column. A route
  whose `handle` is `{ layout: 'workspace' }` (typed as `RouteLayoutHandle` in `src/router.tsx`) gets the whole
  content area and manages its own scrolling instead; the Coding page does.

Every page works at 375px. The shell pads `main` and the topbar `px-4` below `md` and `px-10` from it. Below `sm`
every page's titlebar stacks onto several lines, its buttons full width with their text centered (see Page header).

### Page header

Every page opens with the standard header from `src/layout/page/` (`~/.claude/docs/ux.md`, Standard page layout).
A page in the padded `main` renders inside `Page`, which draws the header and holds the page's one Save:

```tsx
<Page
  title={project.name}
  // Pages above this one, outermost first. Omit on a top-level page.
  parents={[{ label: t('title'), href: UrlTree.projects }]}
  // Optional: the title becomes its own editor.
  rename={{ label: t('page.renameLabel'), maxLength: NAME_MAX_CHARACTERS, onSave: rename }}
  // Optional: leads the title, a `?` popover beside it, and chips after it.
  icon={<ConnectorLogo kind={kind} />}
  help={t('page.help')}
  badges={<Chip>{state}</Chip>}
  // Optional: a list's own search, then the one action that stands on its own.
  tools={<ListSearchField label={t('toolbar.search')} value={search} onChange={setSearch} />}
  primaryAction={<Button>...</Button>}
  // Everything else the page can do.
  actions={[
    { id: 'removeCover', label: t('page.removeCover'), icon: LuImageOff, onAction: removeCover },
    { id: 'delete', label: t('common:actions.delete'), icon: LuTrash2, isDanger: true, onAction: confirmDelete },
  ]}
>
  ...
</Page>
```

- **Breadcrumbs.** With `parents`, the header opens with an icon-only back button, a link to the last parent, then
  the breadcrumbs ending in the title. A top-level page has neither.
- **Titlebar.** The large title on the left, everything else on the right, centered on one line, in this order:
  `tools`, the page's list controls, `primaryAction`, a lone action's button, Save, then the Actions menu, so Save
  keeps the top right corner unless the menu is there. There are no subtitles; a page that truly needs explaining
  puts it behind `help`, a `?` opening `HelpPopover` (`src/components/`), which a section or message whose detail
  only some people need uses too (the new browser connector's sign-in, a stream the media ports block).
- **List controls.** A `Page` holds a slot after `tools` for its list (`pageTools.ts`): the page's list table
  renders its search, filters button, Columns, and export there through uikit's `controls`
  (`~/.claude/docs/ux.md`, Table layout). `usePageToolsContainer` hands the slot to anything inside the page, so a
  list in a child route, such as All items under the Action items tabs, reaches the titlebar its layout draws.
  Calling it claims the slot, which the titlebar draws only while a list claims it, so only the page's own list
  calls it; a table that is one section of a page keeps its Columns and export above itself.
  A list shown as tiles too searches with `ListSearchField` in `tools` instead, which looks and behaves like the
  table's: narrow until typed into, wider after, and focused by Ctrl+F.
- **Drawn from React state.** The titlebar's right side is rendered only while something is in it: `tools`, a list
  that claims the slot, `primaryAction`, Save, or `actions`. Whatever a page gives it after its first render, such
  as a Save registered once its data loads, shows at once, so a page renders its header from the start and its
  loading state below it. Nothing hides the right side with CSS that inspects its content (`:has()`), which Firefox
  does not re-check as buttons arrive.
- **Actions.** `actions` is data (`PageAction` in `pageActions.ts`), so the rule lives in one place,
  `arrangePageActions`: a lone harmless action is its own outlined button, and anything more is one Actions menu on
  the far right, each item with its icon and destructive ones (`isDanger`) in red below a divider. A destructive
  action is never a bare button, even alone. An action acts (`onAction`), goes somewhere (`href`, a real link), or
  opens a submenu (`items`); `isPending` shows on its button or on the menu's trigger.
- **Phones.** Below `sm` the right side stacks under the title and every control in it spans the full width,
  centered (`.page-actions` in `src/index.css`).
- **Save.** A page with a form carries its one Save in the header; see Saving.
- **Creating.** Every create flow is a page of its own under `/new`, never a dialog (`~/.claude/docs/ux.md`, Sub-pages
  and Form layout). It has the header's Save, named for what it does (usually Create), and `PageCreateFooter` at the
  end of its form: Back on the left, a real link to the page's last parent (or its `escapeTo`), and on the right the
  same Save under the same label, its second place. Both run the page's one save, so they are always enabled,
  pending, and explained alike; on phones they stack full width, Create on top. A form shared with an edit page
  renders the footer only when creating. A `/new` page whose create needs nothing typed (starting a connector's
  browser, starting a Claude sign-in, a coding session whose only project is preselected) registers its save with
  `canSaveUnchanged`, so Save can be pressed at once while leaving still asks only about real changes.
- **Lists.** A page shows one list (`~/.claude/docs/ux.md`, Lists of data). A page with more puts each on a tab of
  `PageTabs` (`src/components/PageTabs.tsx`), each tab named with its total beside it, zero included, and none while
  it is still loading; a count of what waits on the person (the inbox, sign-ups) is accented while above zero. Tabs
  inside another page's tabs (Changesets' under Action items) pass `level='nested'` and draw as a compact segmented
  control rather than a second underline. The
  open tab lives in the address through `useTabParam` (`src/hooks/useTabParam.ts`): `?tab=`, plain for the first tab,
  replacing the history entry, so a tab survives a reload and a link can open one (`getTabUrl`). A form never shares
  a page with a list: one that belongs beside a list is a page of its own, reached from the list page's actions and
  going back to it once saved (Workspace access, Agent configuration, Backup destination).
- **Workspace pages.** A page that manages its own scrolling (the Coding page, a Studio item) renders `PageHeader`
  alone in its top bar, with the same props, and passes `className` for its spacing. It has no Save, so nothing on it
  is a field: a Studio item is renamed from its Actions menu, in a dialog with its own button.

### Saving

Elysium saves manually, per page (`~/.claude/docs/ux.md`, Saving). No field saves itself as it changes: a page's
fields wait for its one Save, and every save is confirmed with a toast. What acts on its own is an action (a button
or menu item that does one thing, such as Resolve, Approve, Link, Forget, or Delete), never a field.

- **Registering.** The form that owns the page's fields registers the page's Save, however deep it sits, with
  `usePageSave` (`src/layout/page/pageSave.ts`), which returns the form's `onSubmit`:

  ```tsx
  const onSubmit = usePageSave({
    // Resolves true once saved, false when invalid or refused; the form reports a refusal itself.
    save: saveForm(form, async (values) => { ...; return true }),
    isDirty: form.formState.isDirty,
    // Optional: why Save is disabled, shown in its tooltip; and a label other than Save.
    blockedReason: null,
    label: t('actions.connect'),
  })
  return <Form onSubmit={onSubmit}>...</Form>
  ```

  `saveForm` runs a react-hook-form form's validation, then its handler. A page whose own component holds the form,
  because its fields sit in the header and beside other sections (an item, an initiative, a project), passes the same
  object to `Page` as `save` instead.
- **One path.** `Page` runs the save for every way of saving: the header's Save, the floating card, Ctrl+S, Enter in
  the form, and Save and continue. It tracks the save's progress itself, so Save shows pending and nothing runs it
  twice. Save is enabled while the form is dirty and not blocked; disabled, its tooltip says why, "Nothing to save
  yet" when nothing changed. Forms keep no save button or cancel button of their own, except a `/new` page's Create.
- **Floating Save.** While the header's Save is scrolled out of view and the form is dirty, `FloatingSaveCard` offers
  it in the bottom right, unless a `/new` page's Create below the form is in view. Dismissing it lasts until the
  header's Save is back in view.
- **Leaving.** While the form is dirty, every way out of the page (a link, the sidebar, the breadcrumbs, Esc, the
  browser's Back, or a change of the page's query other than its tab, which can swap what it edits; switching tabs
  keeps the form mounted, so it never asks, `swapsWhatPageEdits`) is held by React Router's
  `useBlocker` and asks in `LeavePageDialog`, with a warning icon: Discard work, Save and continue, or Keep editing.
  Discard work calls the page save's `onDiscard` first, which ends a `/new` form's draft (see Forms, Drafts).
  Enter saves and continues; Esc keeps editing. A save that fails stays on the page. Closing or reloading the tab gets
  the browser's own warning. A form's own navigation after it saves (a `/new` page opening what it created, an edit
  page going back to its list) goes through without asking, and during Save and continue it is held, so the person
  still lands where they were heading. Leaving a record just deleted passes `LEAVE_WITHOUT_ASKING` as the navigation's
  state. The blocker needs a data router, so tests that render a `Page` use `createMemoryRouter`.
- **Keyboard.** Esc goes up a level: to `escapeTo`, or on a page with a Save to its last parent, and the leave prompt
  asks first when the form is dirty. It is left alone when a component already used it (`defaultPrevented`, as an
  inline field putting its text back does), inside a menu or list box, and while a modal is open, so a modal closes
  first. Ctrl+S (Cmd+S) never opens the browser's save dialog on any `Page`, and presses Save when it can. The rules
  are `readPageShortcut` in `src/layout/page/pageShortcuts.ts`.
- **Records edited in place.** An item, an initiative, and a project keep their fields in one form
  (`useActionItemEditor`, `useInitiativeEditor`, `useProjectEditor`): the title in the header (`rename`), the notes
  or description, and the side panel's fields. `InlineEditableText` reads as text until clicked and edits the form
  value as it is typed, so Save and Ctrl+S always see it. Fields not touched follow the record as the event stream
  changes it, and a save sends only what differs from the record as it stands, joining and leaving projects and
  initiatives one route each; a save that fails part way is finished by the next.
- **Feedback.** A save is confirmed with a toast alone. Nothing a save reports may move the page, so there are no
  success banners above the form.

`AppShell` also mounts HeroUI's `RouterProvider`, wired to React Router's `navigate` and `useRouterHref`
(`src/layout/useRouterHref.ts`). That makes any HeroUI `Link` `href` a client-side navigation, while a full URL to
another site keeps its own address. A link's `routerOptions` are React Router's own (`src/@types/react-aria.d.ts`),
so a link can carry state, such as `LEAVE_WITHOUT_ASKING` for one that stays on its page. It also mounts the toast
region.

Anything that goes somewhere is a real link, so it can be middle-clicked, ctrl+clicked, or opened in a new tab
(`~/.claude/docs/ux.md`, Buttons): a button is a HeroUI `Link` styled with `buttonVariants(...)`, a menu item is a
`Dropdown.Item` with an `href`, a page action has an `href`, and a table row that opens something has a link in its
title cell. `navigate()` is kept for navigation that is not a link: after a save or a delete, a keyboard shortcut, and
a click anywhere on a row, which only adds to the row's own link.

## Routes

Paths live in `UrlTree` in `src/urls.ts`, and the route table lives in `src/router.tsx`. `/` redirects to Action
items, the first page in the sidebar; there is no home page.

An unknown path goes one level up, to its nearest parent page (`getUnknownRouteRedirect` in `src/unknownRoutes.ts`):
`/settings/x` to `/settings`, `/settings/llms/x` to `/settings/llms`. Parents with a parameter are skipped, since the
record may not exist, so `/studio/x/y` lands on `/studio`, and a record whose page is under `/edit`, such as
`/settings/environment/:variableId`, lands on its list. A path with no parent page goes to Action items. A new page
needs nothing for this beyond its `UrlTree` entry.

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
| `/action-items/changesets`    | `ChangesetsPage`         | Changesets waiting for review and reviewed, as tabs in `?tab=` |
| `/action-items/changesets/:changesetId` | `ChangesetPage` | One changeset's review, outcome, and undo         |
| `/projects`                   | `ProjectsPage`           | Projects as a table or tiles, searched and sorted        |
| `/projects/new`               | `CreateProjectPage`      | Create a project, with its cover                         |
| `/projects/:projectId`        | `ProjectPage`            | One project, edited in place, with its lists as tabs in `?tab=` |
| `/action-items/initiatives/:initiativeId/containers/new` | `NewInitiativeContainerPage` | Follow a Jira epic or filter, or a GitHub milestone or label |
| `/coding`                     | `CodingPage`             | Dockview workspace; `?item=` redirects to `/coding/new?item=` |
| `/coding/new`                 | `NewCodingSessionPage`   | New session; `?item=` starts it from that action item    |
| `/coding/:sessionId`          | `CodingPage`             | Opens session number `sessionId`, then returns to `/coding` |
| `/studio`                     | `StudioPage`             | Studio items as tiles, filtered from the address         |
| `/studio/new`                 | `NewStudioItemPage`      | New Studio item, which opens once created                |
| `/studio/:itemId`             | `StudioItemPage`         | One item: the stage beside its conversation, live or deleted |
| `/runs`                       | `RunsPage`               | Workflow runs, newest first, a page at a time; members only |
| `/runs/:runId`                | `RunPage`                | One run: its steps, each opening to its log, and its history |
| `/search`                     | `SearchPage`             | Every search result for `?q=`, as the topbar's View all opens it; `?tab=` one kind |
| `/settings`                   | `SettingsDirectoryPage`  | Stripe-style directory; reached from the topbar gear     |
| `/settings/account`           | `AccountPage`            | My account: name and email, and the light, dark, or system theme |
| `/settings/profile`           | redirect                 | To `/settings/account`                                   |
| `/settings/personal-details`  | redirect                 | To `/settings/account`                                   |
| `/settings/security`          | `SecurityPage`           | Security: password, two-factor, passkeys, backup codes, sessions |
| `/settings/security/two-factor/new` | `AddTwoFactorPage` | Add 2FA: name, backup codes, then the app; `?step=` resumes it |
| `/settings/security/two-factor/edit` | `ChangeTwoFactorPage` | Rename the app, or move to a new one        |
| `/settings/security/two-factor/remove` | `RemoveTwoFactorPage` | Remove 2FA, and its backup codes          |
| `/settings/users`             | `ManageUsersPage`        | People and sign-ups as tabs in `?tab=`; admins           |
| `/settings/users/access`      | `WorkspaceAccessPage`    | Whether anyone can sign up and must use an authenticator app; admins |
| `/settings/backups`           | `BackupsPage`            | Where backups go, the next one, and every attempt; admins |
| `/settings/backups/destination` | `BackupDestinationPage` | The storage location backups go to; admins             |
| `/settings/browsers`          | redirect                 | To `/settings/connectors`                                |
| `/settings/browsers/:browserSessionId` | `BrowserSignInRedirect` | To the connector that owns the browser, else the list |
| `/settings/llms`              | `ManageLlmsPage`         | List, activate or deactivate, and delete LLMs            |
| `/settings/llms/new/:provider` | `AddLlmPage`            | One per provider, see below                              |
| `/settings/llms/add-<provider>` | redirect               | To `/settings/llms/new/<provider>`                       |
| `/settings/llms/:llmId/edit`  | `EditLlmPage`            | Edit a credential; its provider type is fixed            |
| `/settings/llms/agent-configuration` | `AgentConfigurationPage` | The repository agents' Claude Code configuration comes from; admins |
| `/settings/models`            | `ModelSettingsPage`      | The model and effort each step and agent runs on; admins |
| `/settings/email`             | `ManageEmailPage`        | Mailboxes and domains as tabs in `?tab=`                 |
| `/settings/email/server/new`  | `NewMailServerPage`      | Create the mail server, or try again after it failed     |
| `/settings/email/domains/new` | `NewMailDomainPage`      | Add a domain to the mail server                          |
| `/settings/email/mailboxes/new` | `NewMailboxPage`       | Create a mailbox on the mail server                      |
| `/settings/email/:accountId/edit` | `EditMailboxPage`    | A mailbox's sender name and whether it is active         |
| `/settings/environment`       | `ManageEnvironmentPage`  | Environment variables every coding session receives     |
| `/settings/environment/new`   | `AddEnvironmentVariablePage` | Add an environment variable                          |
| `/settings/environment/:variableId/edit` | `EditEnvironmentVariablePage` | Edit an environment variable              |
| `/settings/connectors`        | `ConnectorsPage`         | Every connector, its status, and activity               |
| `/settings/connectors/new`    | `ConnectorPickerPage`    | A searchable grid of every kind, each opening its form; `?provides=` narrows it |
| `/settings/connectors/new/:kind` | `NewConnectorPage`    | One kind's form: sign in, what it reads, live checks; a personal kind's help says everyone can search what it reads |
| `/settings/connectors/:connectorId` | `ConnectorPage`    | One connector: status, its browser to sign in on, content, and checks |
| `/settings/github`            | redirect                 | To `/settings/connectors`                               |
| `/settings/github/new`        | redirect                 | To `/settings/connectors/new`                           |
| `/settings/github/:credentialId/edit` | `EditGithubCredentialPage` | Edit a GitHub token                            |
| `/settings/jira`              | redirect                 | To `/settings/connectors`                               |
| `/settings/jira/new`          | redirect                 | To `/settings/connectors/new`                           |
| `/settings/jira/:credentialId/edit` | `EditJiraCredentialPage` | Edit a Jira site                                   |
| `/settings/jira/:credentialId/done-transitions` | `JiraDoneTransitionsPage` | A project's done status; `?project=` picks it |
| `/settings/meetings`          | `MeetingsPage`           | Recent meetings and meeting sources, as tabs in `?tab=`  |
| `/settings/company-doc`       | `CompanyDocPage`         | The company doc, edited in place, its proposals beside each section, and its history; members |
| `/settings/learnings`         | `LearningsPage`          | Learnings, the deleted ones, and for admins the feed, as tabs in `?tab=` |
| `/settings/learnings/new`     | `NewLearningPage`        | Save a learning; a takeaway one already has revises it   |
| `/settings/learnings/:learningId` | `LearningPage`       | One learning, corrected in place, with its history and Undo |
| `/settings/learning-categories` | `LearningCategoriesPage` | Every learning category, the workspace's own reordered and removed from their rows; admins |
| `/settings/learning-categories/new` | `NewLearningCategoryPage` | Add one of the workspace's own categories; admins |
| `/settings/learning-categories/:categoryKey/edit` | `EditLearningCategoryPage` | Rename or describe one; under `/edit`, since a key may be `new` |
| `/settings/storage`           | `ManageStoragePage`      | Storage locations Elysium saves files to                 |
| `/settings/storage/new`       | `AddStorageLocationPage` | Add a storage location                                   |
| `/settings/storage/:locationId/edit` | `EditStorageLocationPage` | Edit a storage location                         |

The settings directory groups entries into titled sections, each a responsive grid of `SettingsDirectoryItem`s: Personal
settings, Knowledge base (meetings; the company doc and learnings, hidden from guests; learning categories, for admins
alone; and the knowledge base's own settings as they are built), and Workspace settings. Each entry is a tile, its icon
and title and nothing more, all of it one React Aria `Link`: borderless at rest, it shows its box on hover and on
keyboard focus. Every entry has an icon of its own.
New settings pages add an entry to a section and a route under `/settings`, and render inside `SettingsPage`.

### Settings pages

Every settings subpage renders inside `SettingsPage` (`src/pages/Settings/SettingsPage.tsx`): a `Page` (see Page
header) under Settings in the breadcrumbs, whose Esc goes back to the parent page. It takes `Page`'s props, so a page
only describes itself:

```tsx
<SettingsPage
  title={t('page.editTitle', { name })}
  // Pages between Settings and this one, for the breadcrumbs, the back button, and Esc. Omit at the top level.
  parents={[{ label: t('title'), href: UrlTree.settingsLlms }]}
  // Optional, left of Save, such as a list page's Add button.
  primaryAction={<Button variant='outline'>...</Button>}
>
  <LlmForm />
</SettingsPage>
```

- **Header.** The standard page header, with Save between the primary action and the Actions menu.
- **One Save per page.** The form that owns the page's settings registers it with `usePageSave`, as every page does
  (see Saving). Esc goes to the last parent, or `/settings`. A page with nothing to save, such as a list, registers
  nothing and shows no Save.

## Accounts

The account pages sit outside the app shell: `/login`, `/signup`, `/recovery`, `/pending`, and `/auth/error`. They
render Ory Kratos's self-service flows through its JSON API at `/api/identity/`, using `@ory/client-fetch`
(`src/api/kratos.ts`, `src/api/kratosFlows.ts`). Passkeys go through `@simplewebauthn/browser`, and the email field
offers them in autofill. See `docs/auth.md`.

- **What a submit reports.** A field's refusal shows on the field. Kratos's messages for the whole form, and a
  flow that expired or a request that failed, are toasts (`useKratosMessageToasts`), each set replacing the last, so
  a retried refusal never stacks and nothing moves the form. A form that cannot load says so in its place. A
  recovery link once made turns the page into its next step: who to ask for the link.

- **The gate.** `src/auth/AuthGate.tsx` wraps every workspace page. It loads `GET /api/v1/me` into `authSlice` and
  sends people where they must go first: sign-in, the second factor, `/pending`, the disabled notice, or Security
  when the workspace requires an authenticator app.
  - **Refusals.** Any request's `401` or coded `403` lands in Redux through the ky hook in `src/api/index.ts`, and the
    gate routes on it the same way.
  - **Event stream.** The stream opens only once the person may use the workspace, and closes on sign-out.
  - **Ending sessions.** Sign out (`src/auth/signOut.ts`) and the Sessions tab's End buttons ask the API, never Kratos,
    so the ended session is refused at once (`docs/auth.md`, Ending a session). The tab still lists from Kratos.
- **The waiting page.** `/pending` asks `/me` every 5 seconds. It is the one page that polls: a pending person may not
  open the event stream.
- **Kratos settings flows.** My account and Security each run one through `useKratosSettingsFlow`
  (`src/hooks/`), named with the page a change needing a recent sign-in sends the person back to, from `/login` in
  refresh mode. A field's refusal shows on the field; Kratos's messages for the whole form become toasts, and the
  success it reports after every save is dropped for the page's own toast, so nothing moves the page.
- **My account** (`/settings/account`). First and last name side by side (`PersonNameFields`, also on sign-up), the
  email below them, saved to Kratos by the page's Save; then the theme, which applies as soon as it is picked.
- **Security** (`/settings/security`). Tabs, the open one kept in `?tab=`: General, Passkeys, Backup codes (Kratos's
  lookup codes), and Sessions, where lists show their totals beside their names.
  - **General.** A Password card whose Change password opens `ChangePasswordModal`, which asks for the new password
    twice. A Two-factor authentication card showing whether it is on, the authenticator app by its nickname, and
    whether backup codes are set up, with Add 2FA, or Change 2FA and Remove 2FA, each a link to its own page.
  - **Passkeys, Backup codes, Sessions.** Each list with its action above it. Codes on show copy and download as one
    file, as they do while 2FA is added (`BackupCodesList`).
- **Two-factor pages.** Each opens with `useRecentSignIn`, which sends someone whose sign-in has less than five
  minutes of Kratos's 15 left to confirm it's them first, coming back to the page. How each change reaches Kratos is in
  `docs/auth.md`, Authenticator apps. Security and these pages are where someone the workspace requires to set up an
  app may be.
  - **Add 2FA** (`/two-factor/new`). Three steps with a clickable progress bar (`StepProgress`): the app's nickname,
    then its backup codes, then the QR code and key with the six-digit code. Next on the codes step stays disabled
    until they are downloaded, in one click, as a text file named for the app and the local time
    (`backupCodesFile.ts`). The step, the name, and both Kratos flows' ids are in the address
    (`useKratosSettingsFlow`'s `flowParam`), so a reload resumes with the same unsaved codes. The page's Save is Turn
    on 2FA, in the header and below the last step. Moving between steps never asks about leaving; leaving the page
    does, and cancels.
  - **Change 2FA** (`/two-factor/edit`). The nickname, saved by the page's Save, and Move to a new app, which shows
    the new app's QR code and key from the API and switches once its code is entered; the current app works until
    then.
  - **Remove 2FA** (`/two-factor/remove`). What stops, with "Also turn off backup codes" on by default, and Remove 2FA
    enabled after a five-second countdown.
  - **Copying.** The setup key and backup codes copy with `CopyButton`, an icon-only button that confirms with a
    toast.
- **Users** (`/settings/users`). Admins only. Two tabs: People, everyone approved, and Sign-ups, those waiting for
  an admin. Each person's role is a field of the page, kept by its Save; a role the API refuses, such as the last
  admin's, stays picked. Approving, disabling, and the rest of a row's actions act at once. Workspace access
  (`/settings/users/access`), opened from the page's actions, holds the sign-up and authenticator switches with a Save
  of its own, and goes back to Users once saved. Users flow through SWR, then `usersSlice`, then the `user.*` events.
  The directory entry is hidden from others and badges the pending count.
- **Updates** (`/settings/updates`). Admins only. The release running, auto-update's state with what it waits for, a
  warning when no updater is connected, Check now, Update now for a release that rolled back, and the update history.
  The status flows through SWR, then `updatesSlice`, then the `updates.status` event. The directory entry is hidden
  from others.
- **Backups** (`/settings/backups`). Admins only. Where the databases are backed up to, the last backup stored, the
  next one or that backups are off, and how long they are kept, then every attempt, with Back up now and Change
  destination in the header. The destination is its own page (`/settings/backups/destination`): a storage location,
  or Nowhere, which turns backups off, kept by its Save, going back to Backups once saved. The status flows through SWR, then `backupsSlice`, then
  the `backups.status` event. The directory entry is hidden from others. See `docs/infrastructure.md`, Backups.
- **Password field.** `PasswordInput` is the one field that sets a password, on `SecretInput`. It shows a zxcvbn
  strength meter (`@zxcvbn-ts/core`, with dictionaries loaded only where the field is) and every requirement as a
  checklist: 12 or more characters, an uppercase letter, a symbol, and hard to guess (a score of at least 3). A met
  one is green with a check, an unmet one grey with a red X. Forms stay disabled until all are met. Sign-up and Change
  password ask for it twice, and say so under the second field once the two differ. Signing in has the same reveal
  toggle.
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
  - Tabs (`PageTabs`, the open one in `?tab=`): the person's connected clients, with Disconnect, and, for admins,
    every registered client, with Delete. Each tab carries its count, so both lists load with the page rather than
    when a tab opens.
  - This data is loaded with SWR and revalidated after a change; no event stream carries it.
  - The page is named for the protocol, so its address is too. `/settings/connected-apps`, its earlier address,
    redirects to it, so links and bookmarks to it still land.

See `docs/mcp.md`.

## Pages and components

### Icon buttons and menus

- **Icon-only buttons** (`~/.claude/docs/ux.md`, Brevity) are HeroUI's `isIconOnly`, named by `aria-label`, and the
  trigger of a `Tooltip` that shows the same name: the button is the tooltip's first child, never inside
  `Tooltip.Trigger`, which is a tab stop of its own. Only a button that is disabled, and so fires no hover, sits in
  a wrapper for its tooltip (`PageSaveButton`). Copying is `CopyButton`, beside anything people paste elsewhere:
  commands, client ids, hostnames, link keys, and a tool's input and output in a conversation.
- **Working buttons** (`~/.claude/docs/ux.md`, Buttons). A button whose press waits on a request sets `isPending`,
  and shows a spinning ring in place of its contents at its full color (`.button[data-pending]` in
  `src/theme/matter.css`): HeroUI 3.2 only marks a pending button and stops presses, drawing nothing. A component
  with several such buttons tracks which one runs with `usePendingAction` (`src/hooks/`): that one is pending, the
  others are disabled, and a press while one runs is ignored. The connector hub tracks running actions per connector
  (`useConnectorActions`), so one connector's sync never holds another's. The ring can only hide elements, so a
  Button's text is always in a span; `src/bugs/187.test.ts` fails on text written straight into one.
- **Menus** (Dropdowns). Every item leads with its icon; destructive items are last, below a `Separator`, red, and
  fill red under the mouse or keyboard (`.menu-item--danger` in `src/theme/matter.css`), and each asks first or
  offers Undo. A trigger with text ends with a chevron.
- `src/bugs/171.test.ts` reads every source file and fails on an icon-only button without a name or a tooltip it
  triggers, and on a dropdown item without an icon.

### Hotkeys

How the keys fit together (`~/.claude/docs/ux.md`, Hotkeys). Each has one owner, so no two answer the same press.

| Keys            | Where                                | What it does                                                    |
|-----------------|--------------------------------------|-----------------------------------------------------------------|
| `/`             | Anywhere outside a text field        | Focuses the topbar's global search (see Global search)          |
| Ctrl+F, Cmd+F   | Outside a text field, no modal open  | Focuses the page's own search, if it has one                    |
| Ctrl+S, Cmd+S   | Any `Page`                           | Presses the page's Save; never the browser's save dialog        |
| Esc             | A page, outside a menu or modal      | Goes up a level, asking first about unsaved changes             |
| Enter           | A dialog or composer form            | Submits it, except on a button, a list, or in a text area       |
| Ctrl+Enter      | A text area in such a form           | Submits it; Enter alone adds a line                             |
| Letters         | Next and Inbox, outside fields       | Quick actions such as Resolve `R` (see Action items)            |

- **Ctrl+F.** A page has at most one search that takes it: uikit's table search through its `focusHotkey` (on by
  default), or, for a list that is not only a table, `ListSearchField`, whose table hides its own search
  (`search: { show: false }`), so the table's listener is never mounted. Searches outside a table (the search
  page, the connector picker, the connector list) use `useFindHotkey` (`src/hooks/`), as `ListSearchField` does; it
  takes a flag to focus the search as the page opens too. Every listener leaves to the browser's own find a press
  another search took (`defaultPrevented`), one typed in a text field, and one made while a modal is open; a page
  with no search leaves it to the browser as well.
- **Enter in forms.** A dialog's fields sit in a `Form` whose `onKeyDownCapture` is `submitOnEnter`
  (`src/components/submitOnEnter.ts`), so Enter submits through the form's `onSubmit` from a text input, a switch, a
  checkbox, or a date's segments alike: Link target, Snooze, Elysia's hours, Annotate, the prompt gate, and Report.
  It listens on the way down, because React Aria's date fields stop Enter on its way up, and it leaves Enter to a
  button, a link, a list box, a menu, a calendar, and an open combobox, which use it themselves. The form's
  `onSubmit` checks that it may go ahead (something picked, nothing pending), since submitting this way skips the
  browser's check of a disabled submit button. A destructive confirmation never uses it: `ConfirmGate`'s `danger`
  tone blocks Enter (see Confirm and prompt gates).
- **Text areas.** Every text area that submits does so on Ctrl or Cmd with Enter through the same handler, Enter
  alone adding a line: the coding session and Studio prompts, Elysia's conversation and answers, comments, replies,
  the report note, and a multi-line prompt such as editing a comment. A page's own form saves with Ctrl+S instead,
  and `InlineEditableText` finishes on Enter, or Ctrl+Enter when multi-line. `src/bugs/178.test.tsx` covers these.

### Global search

The topbar's search (`src/search/`) is Elysium's one way to reach anything (`~/.claude/docs/ux.md`, Global
searchbars); there is no separate command palette.

- **Keys.** `/` outside a text field focuses it. Ctrl+F is left to the page's own search (see Hotkeys).
  While typing, the arrow keys move through the results (React Aria's `Autocomplete` over a `Menu`, focus staying in
  the field), Enter opens the one highlighted, the first by default, and Esc clears the field, then leaves it; an Esc
  it used never reaches the page's Esc (`readPageShortcut`).
- **Results.** The closest five, in sections by kind (Pages, Actions, then the records the API finds: Action items,
  Projects, Coding sessions, Initiatives), each with its icon and a quieter line (the page it sits under, a record's
  state, a session's number), then View all, a link to `/search?q=`, and Ask Elysia about what was typed. Nothing
  matching says so, above Ask Elysia; records the API could not search say that instead. Choosing a result, clicking
  anywhere else, or tabbing away clears it. Every result that goes somewhere is a real link; an action runs in place,
  and one that needs it asks first, as Sign out does. On phones the results span the screen under the topbar.
- **The search page.** `SearchPage` shows every result for `?q=`, asking the API for the most of each kind it answers
  (50). `PageTabs` with `useTabParam` gives a tab for all of them, in sections as the topbar shows them, and one for
  each kind, each counting its results once the records have arrived; the open tab is `?tab=`. Typing replaces the
  address, keeping the tab, so Back leaves the page.
- **Registering.** Each part of the app lists its own pages and actions in a `searchEntries.ts` beside it, exporting
  `searchEntries: SearchEntry[]`; `searchRegistry.ts` gathers every such file with `import.meta.glob`, so a new page
  shows up by adding that file, never by editing a central list. Those files hold only links, icons, and translation
  keys, since they load with the app. An entry is a page (`href`) or an action (`href` to a create page, or `run`,
  which receives the store's `dispatch` and the confirm dialog). Links are parameterless `UrlTree` paths, with a query
  where one opens a tab (`getSecurityTabUrl`). `searchRegistry.test.ts` checks ids, links, and keys.
- **Words.** An entry's title and the page it sits under reuse its area's own keys, spelled with their namespace;
  hidden tags, one string of words per entry, are in its translations (`search:tags.*` today), so each language lists
  its own synonyms: "2FA" finds Two-factor authentication, "log out" finds Sign out. `audience` (`members`, `admins`)
  leaves out what the viewer's role cannot open, as the sidebar and the settings directory do, and `when` what does
  not apply to their account: Add 2FA only while two-factor authentication is off, Change 2FA and Remove 2FA only
  while it is on, read from `/me`'s `hasSecondFactor`.
- **Matching.** `searchMatch.ts`: every typed word must begin a word of the title, the tags, or the parent, accents
  and punctuation ignored; the title counts most, then tags, then the parent, and a title that begins with what was
  typed ranks first. Ties read alphabetically (`searchRanking.ts`). The API matches records by the same rule on their
  titles, accents aside (`docs/api.md`, `/api/v1/search`).
- **Sources.** `useGlobalSearch(query, recordLimit)` merges every source into one ranked list. A source answers
  `{ results, isLoading, hasFailed }` (`SearchSourceAnswer`) with results in the shared `SearchResult` shape. The
  registry answers at once in the browser. `useRecordSearch` asks `GET /api/v1/search` 300ms after typing stops
  (`RECORD_SEARCH_DELAY_MS`), aborts the request a newer query replaces, and never retries, so an older answer can
  never land over a newer one; it reports loading from the keystroke until its own query's answer arrives. Its hits
  become results in `recordSearch.ts`, scored by their titles with the registry's own `scoreSearchMatch`, so every
  source ranks alike, and a kind of record the build does not know is logged and left out. While any source loads,
  the field's magnifying glass is a spinner and the results show Loading…. Each kind takes a `SearchGroup` of its own,
  named in `searchGroupLabelKeys`.
- **Ask Elysia.** `useAskElysia` opens her panel on the conversation (`elysiaSlice`'s `panelView`) and sends what was
  typed as the person's message, which queues her turn. A message that fails to send says so in a toast and stays
  typed in the search.

### Confirm and prompt gates

`src/gates/ConfirmGate.tsx` and `src/gates/PromptGate.tsx` wrap the router in `main.tsx`, each holding one dialog for
the whole app. `useConfirm()` and `usePrompt()` return a function that opens it with options, so a page describes
the question and what happens next instead of owning dialog state:

- `confirm({ title, message, tone, isIrreversible, switches, confirmText, canConfirm, onConfirm })`.
  - `tone: 'danger'` shows a danger icon and button, and Enter never confirms it, even with the button focused.
    Otherwise the dialog opens with the confirm button focused, so Enter confirms.
  - `isIrreversible` is for what can't be undone, such as a permanent delete. The dialog is `danger`, says in bold
    that it can't be undone, and holds its button for five seconds (`IRREVERSIBLE_CONFIRM_SECONDS`), counting
    down in its label (`CountdownButton`). The message names what is deleted, since the body is what people read.
  - `switches` are options the action can take, such as also deleting what it would keep, each a switch that starts
    off. `onConfirm` receives the ids left on. A switch with `makesIrreversible` turns the dialog irreversible while
    it is on, starting the count again.
  - `canConfirm: false` turns the dialog into an explanation with only Got it, for an action that is blocked. A
    blocked irreversible one keeps its danger icon, with no countdown and no bold line.
- `prompt({ title, label, defaultValue, description, isOptional, maxLength, isMultiline, onSubmit })` asks for one
  value and passes it trimmed.

Both keep the dialog open, the button pending, while the callback runs. It closes when the callback resolves and
stays open when it throws, so a callback reports its own failure (a toast) and rethrows to let the user retry. Esc
closes either, except while its callback runs. No dialog's button says Close: a dialog with nothing to confirm says
Done or Got it, and one that abandons something says Cancel.

Every permanent delete confirms with `isIrreversible`, as does anything else that can't be taken back: resetting a
browser's profile, turning off lookup codes, removing a passkey or an authenticator, rejecting a sign-up, deleting a
Studio item permanently. Changes that are not deletes (unlinking, disabling an account, undoing a changeset) confirm
with `tone: 'danger'`.

A removal that can be undone never asks (`~/.claude/docs/ux.md`, Delete confirmations): it happens at once, and its
success toast carries Undo.

| What                                     | Undo                                                                   |
|------------------------------------------|------------------------------------------------------------------------|
| Deleting an action item or an initiative | Its restore route, which records the restore in its history           |
| Deleting a Studio item or a learning     | Its restore route                                                      |
| Taking an item out of an initiative      | Joining it again, which its history records                            |
| Removing a project's cover               | The cover's restore route; the API keeps the cover until one replaces it |
| Discarding a reply draft                 | Its text back for the tab (and, while the box is open, where it was going); Send writes a new draft |

- **From the record's own page.** Deleting an item, an initiative, or a Studio item from its page leaves for its list
  (with `LEAVE_WITHOUT_ASKING`, see Saving), and Undo restores it and comes back to its page, wherever the person is
  by then. A learning stays on its page, shown deleted with Restore.
- **After the toast.** Undo lasts as long as its toast, at least 6 seconds. A deleted record is restored after that
  from its list's Deleted filter, or from its own page's Restore.
- **Where it lives.** Each area's delete owns its Undo: `useActionItemActions`, `useInitiativeActions`,
  `useStudioItemActions`, `InitiativeMembers`, `useProjectActions`, and `ReplyComposer`.

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
  controls. Column widths and order persist in localStorage. The name, over its description, links to the edit page;
  a signed-in account's email has its own column, and each usage window (`ClaudeUsageWindow`) is a column of its
  own, its percentage over its bar and when it resets below. A row's actions activate or deactivate it and delete it.
- `AddLlmPage` and `EditLlmPage` are full pages, not modals. Each provider type has its own add page under
  `/settings/llms/new/`, named by its slug in `llmSlugByType` (`llmProviders.ts`): `claude-account`,
  `claude-api-key`, `claude-code-oauth`, `openai-api-key`, and `codex-oauth` (`chatgpt-oauth`). A slug it does not
  know goes back to the list, and `/settings/llms/add-<slug>` redirects to the slug's page. A Claude account's page
  (`ClaudeSignInForm`) has no token to paste: its Save is the step ahead, Start sign-in and then Finish sign-in.
- `LlmEditorLayout` puts `LlmForm` on the left and `LlmSetupChecklist` on the right: the steps for getting that
  type's token as checkboxes, with commands and links. Commands, paths, and URLs are data in `llmProviders.ts`
  and never translated. Checked steps are not saved.
- `LlmForm` handles create and edit. The token field has a reveal toggle. Expiry is a date, not a time: HeroUI's
  `DatePicker` (typed segments plus a calendar) at day granularity, saved as midnight at the start of that day in
  the viewer's zone. One year from now picks the date 365 days after today.

Admins reach Agent configuration (`/settings/llms/agent-configuration`, `AgentConfigurationPage`) from the list's
actions: the https repository every agent's Claude Code configuration is checked out from, in
`AgentConfigurationSettings`, saved to the workspace settings by the page's Save (see `docs/coding.md`). The list's
Add LLM menu sits in its header.

Storage under `src/pages/Settings/Storage/` lists storage locations in a table (name, provider and region, zone or
bucket with directory, projects, limit, and a Default chip on Studio's default). The name links to the edit page, and
a row's actions test or delete one; delete confirms through `useConfirm`. Adding and editing are their own pages: `StorageLocationEditorLayout`
holds the breadcrumbs and title, and `StorageLocationForm` lays out the form beside a `StorageSetupChecklist` for the
chosen provider. A stored location's form also has the Studio's default switch, sent only when changed.

The Provider picker offers three options (`STORAGE_OPTIONS`): Bunny Storage, Amazon S3, and Google Cloud Storage. The
last two are the API's `s3` kind with a `service`, split in the picker because each is set up in its own console. The
form keeps every option's fields and shows the chosen one's: zone and region for Bunny; bucket, AWS region, and access
key ID for S3. `toStorageProvider` builds the API's provider from them through a lookup per option. The secret's label
follows the provider's own word (Password, Secret access key, Secret), and it stays optional while editing until the
settings need a new one. Directory, projects (`ProjectScopePicker`, see `docs/storage.md`), and a limit in gigabytes
(a `NumberField` in the viewer's locale led by GB, beside a No limit switch) are shared. Labels, secret wording, and setup steps
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
page, kept by its Save, with a notice when its token was deleted) and for a session (`CreateSessionForm`). Both offer
following the level above, no token, or a token by name. See `docs/github.md`. The edit page sits under Connectors in
the breadcrumbs, and goes back there once saved.

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
  each time it opens: the one chosen, the only one, or a warning to choose among several, with a select kept by the
  page's Save and Forget, which acts at once. Picking another project with a choice unsaved asks first. These pages
  and the credential's edit page sit under Connectors in the breadcrumbs.
- `JiraConnectionFields` is the connection half both forms render. The token field is optional while editing and turns
  required as soon as the site or the account email changes, mirroring the API.
- `JiraScopeFields` is the allowlist half both forms render: an All projects switch over `JiraScopePicker`, and the
  same for boards. A board's id is a number over the API and a key in the picker, and this is the one place the two
  spellings meet. `jiraPresentation.ts` holds the site URL rule, `keepsStoredJiraToken`, `toJiraScopeSelection`, and
  the table's scope summary. `JiraSetupChecklist` renders `src/components/SetupChecklist.tsx`, as GitHub's and
  Storage's do. See `docs/jira.md`.

Environment variables under `src/pages/Settings/Environment/` follow GitHub's shape: a table (key in monospace, linking
to its edit page, value, description) whose row action deletes one, and add and edit as their own pages built from
`EnvironmentVariableEditorLayout` and `EnvironmentVariableForm`. A secret's value shows only as a Secret chip; a visible
one is in monospace. The form is a key, a Secret switch, the value (a multi-line text area, or a
password field while Secret is on), and a description. The Secret switch says that a secret is hidden from Elysium
and its session logs while the agent can still read it, and the list page's help says the same of every value.
While editing a secret the value is optional and keeps the stored one, and it turns required when Secret is switched
off, mirroring the API. `environmentPresentation.ts` mirrors the API's refused-key rules (`getKeyRefusal`), so a key
Elysium would refuse is flagged before saving. See `docs/environment.md`.

Email under `src/pages/Settings/Email/` has two tabs, Mailboxes and Domains, each with its count, the open one in
`?tab=`. The page loads the mail server and its domains for both, since creating a mailbox needs them.

The Domains tab is `MailServerSection`, which follows the mail server's state from Redux. With no server it links to creating one
(`NewMailServerPage`, `MailServerForm`: the first domain, and a hostname that follows it as `mail.<domain>` until
edited, which goes back to Email's Domains tab once creation starts); while it is created it shows the current step from
`mailServer.updated` events; once ready it lists domains in `MailDomainTable` (the default domain marked in its own
column, whose row action to remove it is disabled), with Add domain (`NewMailDomainPage`, `MailDomainForm`), a remove
confirmation (which only explains while the domain has mailboxes), and `MailDomainDnsModal`, which checks the
domain's records through SWR each time it opens and lists them with copy buttons (`DnsRecordRow`). Each create page
says why when the server cannot take what it creates (`mailServerBlockerKeys`, `getMailboxBlockerKey` in
`mailPresentation.ts`) instead of showing its form.

The Mailboxes tab shows `MailAccountTable`. `MailboxSourceActions` offers Connect Gmail, Connect Outlook, and
Create mailbox, disabling each with the reason: no broker (from `GET /api/v1/mail/capabilities`, read through SWR),
no ready mail server, or no domain. Create mailbox links to `NewMailboxPage`, whose `CreateMailboxForm` picks one of
the server's domains. The Connect buttons are plain anchors to the API's OAuth start route, because the answer is a redirect to the broker; a client-side route change would not follow it.
`useOAuthOutcomeToast` announces the `mailConnected` or `mailError` the callback returns with, then strips it from
the URL. The address links to the mailbox's edit page, and a row's actions test the connection, send a test message,
and disconnect, or delete a self-hosted mailbox; the disconnect confirmation says whether mail is deleted (self-hosted) or only forgotten (OAuth). The edit page
(`EditMailboxPage`, `MailboxForm`) holds the sender name and whether the mailbox is active, kept by its Save.

### Models

`src/pages/Settings/Models/` is where admins choose the model and effort each workflow step and agent runs on
(`docs/engine.md`, Models), in the settings frame; anyone else is told it is for admins, and the directory hides the
entry.

- **The list** (`ModelSettingsPage`, `ModelSettingsTable`) is uikit's `SmartTable`, unsorted, since the order is the
  workflows' own: each step and agent by its words over its name in code (a step's words are the Runs pages',
  `useRunNames`), its kind, its model and effort, and whether it runs on its default or a choice. A step that runs no
  model says so in place of fields.
- **Fields.** Model and effort are the page's fields, kept by its Save, which sends one `PUT` per row that changed,
  keeping a refused one picked, as Users keeps roles. The model is `ModelField`, a combo box suggesting the current
  Claude models by product name that also takes any id or alias typed, such as `opus[1m]`; `isModelName` mirrors the
  API's rule, so one that could read as a flag shows its error and blocks Save. Use default, a row action on a row
  with a choice, acts at once.
- **Data.** SWR, then `modelSettingsSlice`, then `modelSetting.changed`, which carries an agent's name alone and makes
  admins load the list again. `modelSettingsPresentation.ts` holds the rules and labels, tested beside it.

### Learnings

`src/pages/Settings/Learnings/` is the minimal view of learnings (`docs/knowledge.md`, Learnings) until the Knowledge
page folds it in, in the settings frame:

- **The list** (`LearningsPage`, `LearningTable`) is uikit's `SmartTable`: the takeaway, opening the learning; its
  category; who first saved it and through which agent; and when it last changed. Both lists filter by category, any
  built-in one or the workspace's own, kept in `?category=`. Deleted learnings are a tab of their own, and admins get a
  third, the feed (`LearningFeed`): every change to every learning, newest first, read on with Show older. A guest is told the knowledge base is for members, as the API refuses them (`selectIsCompanyMember`).
- **New learning** (`NewLearningPage`, `LearningForm`) saves from the header's Save and opens the learning saved,
  saying whether it was new or revised one whose takeaway it named. What is typed is kept as a draft until it is
  saved (see Forms, Drafts).
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
  unsorted, since the order is the point: its name over its key, whether it is built in, its description, and its
  learnings. Built-in ones are fixed; the workspace's own link to their edit page from their name, and their row
  actions move them up or down and remove them.
- **Removing** (`RemoveLearningCategoryModal`) names the category, says in bold that it can't be undone, and holds its
  button for five seconds with the same `CountdownButton` the confirm gate uses. When learnings are filed under it, it
  asks where they move, and the button waits for an answer too.
- **New and edit** are their own pages over `LearningCategoryForm`, saved from the header's Save. A new category's key
  follows its name (`keyFromLabel`) until the key is typed by hand, and is read-only once saved. What a new one holds
  is kept as a draft until it is saved; a key put back that its name would not make was typed by hand.
  `learningCategoryFormSchema.ts` mirrors the API's bounds.

### Company doc

`src/pages/Settings/CompanyDoc/` is the company doc (`docs/knowledge.md`, The company doc), in the settings frame, for
the company's members; anyone else is told the knowledge base is for members.

- **The page** (`CompanyDocPage`) loads the doc for members, counts the proposals waiting in a chip beside the title,
  explains itself behind `help`, and has Look for updates as its one action, which starts the review and says so in a
  toast linking to its run. The history follows the form.
- **The form** (`CompanyDocForm`) is every section in order, one text area each (`company-doc-<section>`, the first
  focused), saved by the page's Save, which sends only the sections that changed. A section the person has not
  touched follows the doc as it changes, so an approval made here or in another tab shows at once; one they edited
  keeps their text.
- **Proposals** (`ProposalCard`) sit beside the section they would change, from `lg` up, and below it on narrower
  screens: who proposed and through which agent, when, the review's run it came from, its reason, the change as a
  `TextDiff` from the text it was made against, and its quote and source. One made against an earlier version of the
  section says that approving merges both. Approve and Reject act at once with a toast; Approve waits, with a tooltip,
  while the section has unsaved edits, and a `409 proposal_overlaps` says the section changed in the same lines.
- **History** (`CompanyDocHistory`) lists every change newest first, each a sentence naming who and which sections,
  with Undo. It is read from SWR alone, since no other view shows it.
- **Data.** The doc flows through SWR (`useCompanyDocLoader`), then `companyDocSlice`, then `companyDoc.changed`,
  which carries nothing, so the stream revalidates the doc's key and the history's for members. Every write answers
  the doc whole, which replaces the slice's.
- `companyDocPresentation.ts` holds the section labels and hints, the history sentences, and how an agent is named,
  tested beside it.

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
  above that. Every tab carries the total it lists when it opens (`countActionItemsSections`): Next, the inbox, All
  items under its default filter, the active initiatives, and the changesets waiting for review; the Inbox and
  Changesets counts are accented while anything waits. An empty Next says how many items wait on someone or are snoozed.
- **Next is computed in the browser.** `src/store/nextOrder.ts` mirrors the API's Next (which items, and
  `api/src/action_items/next.rs`'s order) and is tested against the same cases, the way `environmentPresentation.ts`
  mirrors the API's key rules; keep the two in step. `selectNextActionItems(state, now)` applies it to every live
  item. A snooze running out sends no event, so time-dependent views read `useNow`, which re-reads the clock every
  minute (`ACTION_ITEMS_CLOCK_TICK_MS`) and fetches nothing. The API's `next` route is not used by the frontend.
- **Inbox** (`InboxPage`, `InboxTriageCard`) takes the inbox oldest first: Accept `A`, Dismiss `D`, Open `O`, with the
  next few titles listed below.
- **All items** (`ActionItemListPage`) is `ActionItemTable` (uikit's `SmartTable`), its search, filters, Columns, and
  export in the titlebar. The filters drawer offers states (the inbox and open by default, shown as a tag like any
  filter), project (or no project), initiative, waiting, snoozed, and deleted items, which lists deleted items instead,
  each with a Restore row action (`useActionItemFilterDefinitions`). Filters live in the address
  (`actionItemFilters.ts`, which also maps them to the drawer's values), with only what differs from the defaults
  written, so a filtered list survives opening an item and coming back. The page applies them itself, so a list that
  nothing matches says so while its tags stay to take a filter off. Selecting rows opens the floating selection bar
  (`useActionItemBulkActions`): Change state (Open, Resolved, Dismissed), Change priority, Snooze (the presets, Pick a
  day, or End the snooze), Add to initiative, Remove from initiative, and Delete, at once with Undo; deleted items
  Restore together. Each is one change through `POST /api/v1/action-items/bulk`, sent in parts of at most
  `ACTION_ITEM_BULK_MAX_ITEMS`; the answer lands in Redux at once (`actionItemsUpserted`), the toast counts what
  changed, and a warning that stays names, by title, an item the change was refused for.
- **Item page** (`ActionItemPage`, which loads the item for `ActionItemView`) edits the title (the page's title) and
  notes in place with `InlineEditableText`, kept by the page's Save (see Saving). `useActionItemPageActions` gives its header the first state change the
  state allows as the primary action (`transitionsByState` mirrors the API's table), and an Actions menu with the
  other state changes, Snooze (a submenu of the presets, Pick a day, and End snooze), Wait on someone, Stop waiting,
  Start a coding session, and Delete. `ActionItemSessions` lists the coding sessions started
  from the item, from the `codingSessions` slice, below its comments. `ActionItemDetailsPanel` holds priority, due date,
  projects, and initiatives, kept by the same Save; projects and initiatives go through their per-id routes, one
  request per one joined or left (`membershipChanges.ts`). A due date is sent only when its day changed
  (`toChangedDayInstant`). A deleted item is read-only under a banner with Restore.
- **Links** (`ActionItemLinks`, under the item's notes) lists the item's links from the `actionItemLinks` slice, the
  primary first, each with its provider, kind, key (opening it in the provider), and state. What the provider says now
  (title, status, assignee, priority, due date) comes from `useActionItemLinkRemotes`, which reads every link live
  through SWR and never enters Redux, since an item holds none of those fields; the recorded title and state show until
  it answers, and a link it could not read says why. A row menu makes a link primary or unlinks it. `PendingWrites`
  lists what a link still owes, with its tries, the provider's last answer, and Stop trying; a Jira close that failed
  links to the project's done status choice.
- **Link pickers** (`LinkTargetPicker`) choose one Jira issue, GitHub issue, or pull request: `LinkCredentialSelect`
  lists every Jira site and GitHub token in one select, keyed `provider:id`. Jira is searched by `JiraIssueSearch`,
  a search field that builds JQL from the words or key typed (`buildIssueSearchJql` in `linkPresentation.ts`) once
  typing rests for 300ms (`useDebouncedValue`, `LINK_SEARCH_DELAY_MS`), its magnifying glass a spinner until Jira
  answers. GitHub is a kind, a repository (`GithubRepositorySelect`, sharing the
  coding form's `['github-repositories', id]` listing), and that repository's open issues or pull requests narrowed by
  typing (`GithubIssueList`). Every list is a `ChoiceList`, which shows loading, the provider's refusal, emptiness, and
  a cut short answer. `LinkTargetModal` wraps the picker for Link on the item page and for From Jira or GitHub on New
  item, which creates the item from the pick with the page's preset project and initiative.
- **Snooze** (`SnoozeMenu` in Next, and the item page's Snooze submenu, both opening `SnoozeDateModal` for a day)
  offers later today (three hours), tomorrow morning, next Monday morning, or a day picked
  from a calendar; a day snooze wakes at 09:00 in the viewer's zone (`SNOOZE_WAKE_HOUR`). **Due and target dates** are
  picked as days and stored as the last millisecond of that day in the viewer's zone, so an item due today is overdue
  tomorrow (`actionItemDates.ts`).
- **Comments** (`ActionItemComments`) post with Ctrl or Cmd and Enter (see Hotkeys). Only the user's own comments offer edit and
  delete; an edited one says so.
- **History** (`HistoryTimeline`) lists entries newest first, each an actor and a sentence built by
  `historyPresentation.ts` from the entry's kind and data, with every field an edit changed. Unknown kinds and
  malformed data still read as something.
- **Initiatives** (`InitiativesPage`, `InitiativeTable`) list name, state, progress, target date, and projects, with
  the same titlebar controls as All items. The drawer filters states (active by default), project (or no project),
  and deleted initiatives, each with a Restore row action; filters live in the address (`initiativeFilters.ts`).
  Selecting rows offers Change state and Delete with Undo, or Restore, through `POST /api/v1/initiatives/bulk`
  (`useInitiativeBulkActions`). Progress is always resolved and total together
  (`InitiativeProgressSummary`), with a bar beside the counts, never a percentage.
- **Initiative page** (`InitiativePage`, which loads the initiative for `InitiativeView`) edits the name (the page's
  title, with its state chip beside it) and the description in place, and Delete is its Actions menu. It edits state,
  target date, and projects in `InitiativeDetailsPanel`. All of them are kept by the page's Save. `BurnupChart` draws total and resolved as steps from the API's burnup, with a legend,
  direct end labels, a crosshair and tooltip that snap to the nearest change (by pointer or arrow keys), and the same
  numbers in a table beneath; its geometry is `burnup.ts`. `InitiativeMembers` lists the items with a row action to take
  one out (at once, with Undo), adds existing items from a search, and links to a new item started in the initiative.
  `InitiativeContainers` lists the epics, saved filters, milestones, and labels it follows from the `initiativeLinks`
  slice, each with when the watcher last read it or why it could not, and whether it held more than the watcher reads.
  Follow opens `NewInitiativeContainerPage`, whose `AddContainerForm` picks one through `ContainerTargetPicker`: a
  Jira epic by search, a saved filter from `JiraFilterList`, or a repository's milestone or label from
  `GithubContainerList`, and goes back to the initiative once it follows it. `containerPresentation.ts` holds
  which kinds each provider has.
- **Changesets** (`ChangesetsPage`) has two tabs, Waiting for review and Reviewed, each with its count, the open one
  in `?tab=`, and each a `ChangesetTable` whose search, Columns, and export sit in the Action items titlebar. Both
  list the summary (opening the review, as a click on the row does), who proposed it, its project, and how many
  changes it holds. Waiting for review counts the decisions so far under the changes and says when it was proposed;
  Reviewed shows the state and when it was decided, over who decided it.
- **Changeset review** (`ChangesetPage`, `ChangesetOperationCard`) has Apply as its header's primary action while
  the changeset waits, Approve all and Reject all, or Undo once applied, as its actions, and who proposed and decided
  it with the tally under the header. It shows each operation as a sentence and, for an edit,
  each field from what it holds to what it would hold (after applying, from what the operation replaced), with its
  reason, quote, and source, and which earlier change it acts on. While the changeset is pending, each change has
  Approve and Reject, the page has Approve all and Reject all, and a change says which others rejecting it also
  rejects (`dependentsOf` mirrors the API's rule). Apply is enabled once every change is decided. After, each change
  shows its decision and outcome with the reason it failed or was skipped; an applied comment or resolve lists the
  provider writes it still owes from the item's links (`OperationProviderWrites`), with the provider's last answer.
  Undo confirms first, saying that a posted comment and a closed issue stay; afterwards each change says what undo
  kept. `changesetPresentation.ts` holds the sentences, labels, and rules, tested beside it, and
  `useChangesetActions` the writes. A history entry a changeset made links to its review (`HistoryTimeline`).
- **Project page** (`ProjectItems`, `ProjectInitiatives`) lists, on its Action items and Initiatives tabs, the
  project's inbox and open items and its initiatives, each with a New button that presets the project, and a link to
  every item of the project in All items.
- **Reply** (`src/replies/`). An item a message raised shows a reply box under its links (`ActionItemReply`, loading
  the record with `GET /timeline?actionItemId=`), and a project's timeline offers Reply on each message
  (`RecordReply`). Both appear only where the record's kind declares `replies` other than `never`
  (`useRecordReplies`), so no page names a provider. `ReplyComposer` picks where a chat reply goes (in the thread or
  the conversation), or for mail the person's own mailbox, pointing at Settings, Email when they have none; it
  continues the record's newest unsent draft from the `drafts` slice, and Send writes or edits that draft and sends it
  (`docs/connectors.md`, Sending). A send that failed shows its reason under the text it kept, with Report. What is
  typed is kept as a draft per record (`reply.<recordId>`, see Forms, Drafts) until it is sent or discarded.
  `draft.changed` reloads a draft by id, and drops one that is gone.
- **Start a coding session**, on the item page and in Next, goes to `/coding/new?item=<id>`
  (`getNewCodingSessionUrl`), New session started from the item; see [Coding](#coding).

The shared `src/components/` pieces are `DayPicker` (a date without a time), `MultiPicker` (tags from a searchable
list), `OptionSelect` (a labelled select over a short list), `EmptyNotice` (what a list shows instead of an empty
table), and `TextDiff` (two versions of a text line by line, added lines green, removed ones red,
long unchanged runs folded; `textDiff.ts`, on the `diff` package, decides the rows).

### Coding

`src/pages/Coding/` is a Dockview workspace with a Sessions overview panel and one conversation panel per open
session. `docs/coding.md` describes the panels, the conversation rendering, and layout persistence. These details
matter when changing it:

- Dockview renders panels through portals, so panels share the page's React tree (Redux, i18n, router) but not its
  props. Page actions reach them through `CodingActionsContext`.
- `/coding/<number>` (`getCodingSessionUrl`) opens that session's conversation once Dockview and the sessions are both
  loaded, then replaces the address with `/coding`; a number no session has goes straight to `/coding`. The project
  page links sessions this way.
- The Sessions panel shows its sessions as a table or as tiles. `SessionsToolbar` is the panel's titlebar for both:
  `ListSearchField`, the result count, the table's filters, Columns, and export, and, on the right, a Table and Tiles
  switch (HeroUI's `ToggleButtonGroup`, as on the Projects page). Search matches the number, title, project, state,
  and last activity as shown, so the table's own search is hidden. The filters drawer filters state and project,
  kept in the Coding page's address (`sessionFilters.ts`). Both views read one `SessionSummary` per session, built
  by `describeSession` in `sessionPresentation.ts` (project name, activity state and its label, last activity in the
  viewer's zone); `searchSessionSummaries` and `filterSessionSummaries` narrow them once for either view. The table
  stays mounted behind the tiles, hidden, because its filters drawer, their tags, and the export serve the tiles too;
  its Columns show only in the table view. The table's columns render and sort from those summaries; last activity
  sorts by the instant rather than the words shown.
- The table's row actions open, rename, and delete a session. Selecting rows in the table view offers Delete for the
  selection, confirmed once with the countdown that deleting can't be undone (`deleteSessions` in
  `CodingActionsContext`). Each session is deleted by its own request: a delete stops the session's turns and removes
  its directory in the workspace, work a database transaction cannot hold, so a bulk route would only loop the same
  requests. One that fails is named in a toast and the rest still go.
- `SessionTiles` is a grid that fits as many 16rem columns as the panel is wide, down to one at phone width. Each
  tile shows the number, title, state chip, project, and last activity, with `SessionRowActions`, a menu of the same
  three, in its corner. The title's link stretches over the whole card, so clicking a tile opens the session as clicking the title
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
- New session is its own page, `/coding/new` (`NewCodingSessionPage`), in the padded frame rather than the
  workspace, with the missing credential warning above its form. It holds `CreateSessionForm`, or, with `?item=`,
  `ActionItemSessionForm`, which loads the item first, since the form presets itself from it: the title, the item's
  projects (`getSessionProjectChoices` in `sessionItemProjects.ts` mirrors the API; an item in one project fixes it),
  and a required prompt. The form sends the prompt with the create, and the API queues it as the first turn; then it
  opens `/coding/<number>`, which focuses the new session's conversation. The header's New session and the empty
  Sessions panel link to the page, and `/coding?item=<id>` redirects to it.
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
- Every tab is `CodingTab`, Dockview's own tab with a translated close button and its tooltip. Once a group's tabs
  no longer fit, as on a phone, `CodingTabsMenu` at the end of its strip lists them all, in place of Dockview's
  overflow list (`disableTabsOverflowList`), which has no name or tooltip; the strip scrolls sideways as well.

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
  `stepJob.changed` reloads a step's log, once it was opened. Workflows, steps, and sources are named in code; the
  pages name each by its words in `runs.json` (`names`, looked up through `runNameKeys` by `useRunNames`), and one the
  web app has no words for yet as its name in code read as words (`nameFromIdentifier`).

### Projects page

`src/pages/Projects/ProjectsPage.tsx` lists projects in a table view (uikit's `SmartTable`) or a tiles view (HeroUI
cards). The titlebar's `ListSearchField` matches name and description, and `ProjectsToolbar`, above either view,
counts the results, sorts by name, session count, or last update, in either direction, and switches views.
`projectListing.ts` does the filtering and sorting once, so switching views keeps the same results in the same order.
The table's own search is hidden, its Columns and export sit in the titlebar beside the search, and its column
headers sort through the same state as the toolbar. The chosen view is remembered per browser under
`elysium.projects.view.v1`; search and sort reset on each visit. The listing only opens projects: clicking a row
(uikit's row highlight, which never shows as highlighted) or a tile, itself a link, goes to `/projects/:projectId`.
The cover thumbnail opts out of the row click with `data-no-row-highlight`, so previewing it does not navigate.

`ProjectPage` loads the project for `ProjectDetails`, which edits it in place. The name is the page's title.
Everything on it that changes the project is kept by the page's Save (see Saving).
`ProjectBanner` runs the cover across the page below the header: double-clicking it, or its Change cover button,
picks an image, shown in place until Save uploads it, with Keep the current cover to drop it; without a cover it is a
single Add a cover image button. Beside Change cover, a Fit and Fill switch sets the project's `coverFit`, which every
rendering of the cover follows. The controls show on hover or focus, and stay while a new cover waits. About, below
the banner, holds the description and who created the project. The name and description are `InlineEditableText`.
Save sends the changed fields first, then uploads the cover; a cover that fails stays picked, so the next Save
uploads it alone. The header's Actions menu (`useProjectActions`) holds what cannot be done in place: Remove cover,
at once with Undo, and Delete, which confirms through `useConfirm` and only explains while sessions remain. Below,
`ProjectLists` holds the project's lists as tabs, each with its total, the open one in `?tab=`: Action items (still
owed), Initiatives, Timeline (`ProjectTimeline`, counted by the API's `total`), and Coding sessions
(`CodingSessionsTable`; a row or title opens the session on the Coding page). Switching tabs keeps unsaved edits.

`src/components/InlineEditableText.tsx` shows a field of the page's form as text that becomes its own editor when
clicked, in the same typography. Every keystroke changes the form. Enter finishes a single line, Ctrl or Cmd with
Enter a multi-line one, and clicking away finishes too, trimming the value; Escape puts back what was there when
editing began, and a required value left empty is put back the same way.

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
variables, so it does not depend on Tailwind or HeroUI. `src/theme/matter.css` points the variables at the palette
through `html .jala-table-theme`, the class every table surface carries, including the tooltips, menus, dialogs,
filters drawer, and selection bar the kit renders into the body.

- **`SmartTable`** takes `managedColumns` built with `createManagedColumns`. Each column needs a label, a search
  value (what the user sees, not raw data), and a `cell` renderer.
- **Labels.** Pass `useSmartTableLabels()` as `labels`. The kit ships English strings, and that hook supplies ours
  from i18next (`common.table`).
- **Row actions.** Every table's per-row actions are `rowActions`, never a column: they float over the right of the
  hovered row as icon-only buttons with tooltips, at most three, the rest in an ellipsis menu, and screens that
  cannot hover get the ellipsis menu on every row. An action's `onPress` gets the TanStack row (`row.original` is the
  record) and may return a promise, which spins its button until it settles. A label is fixed, so an action that
  flips, such as Activate and Deactivate, is two actions with `isVisible`. Actions are buttons and cannot be links,
  so a row's edit or view page is its name's `Link`, a real address that opens in a new tab too.
- **Long cells** clamp to three lines (`cellLineClamp`, a column's `lineClamp`). A click expands the row; tables
  whose rows navigate (action items, initiatives, changesets, projects, and a project's or item's sessions) set
  `cellOverflow: 'modal'`, so a double-click opens a clamped cell instead. Cells leave clamping to the kit: an inner
  `truncate` or `line-clamp` would clip text the kit cannot see overflow, leaving no way to read it.
- **Columns** hold one data point, or two stacked and left aligned, such as a name over its email; never two side by
  side. Each table keeps three lookups by column key: `searchValue` (the text the user sees, which the search
  matches), `sortValue` (the column's accessor: numbers, instants as `Date.parse`, and lifecycle indexes, so a column
  never sorts by its words), and, on tables that export, `exportValue` (what the CSV holds).
- **Pagination.** Every table passes `pagination: { show: true }`, 25 rows a page by default.
- **Controls in the titlebar.** `controls: { container }` renders the search, filters, Columns, and export into an
  element, the page's list slot (`usePageToolsContainer`, see Page header) or the Sessions panel's toolbar, while the
  table keeps their state. A table with `toolbar: { show: false }` still shows its Columns chooser above it.
- **Filters** come from a column's `filter` and from `filters.definitions`, with `filters.values` and
  `onValuesChange` to keep them in the address. They open in a drawer and show as tags above the table. Elysium's
  lists declare them without `matches` and apply them themselves, from the address, so a page knows when nothing
  matches.
- **Export.** `csvExport` adds a one-click CSV of the filtered, sorted rows; `exportValue` gives a column whose
  accessor is a sort key its real value. The data tables export (action items, initiatives, changesets, projects,
  coding sessions, runs, learnings, and meetings); the settings tables do not. A CSV holds states and other labels as words, dates as
  their UTC instants, and leaves images out.
- **Multi-select.** `enableSelection` with `selectionActions` and `bulkEdit` gives the floating selection bar, its
  hotkeys, and select-everything across pages. Phones get none. The kit draws the checkboxes but holds no selection,
  so the table passes `rowSelection` and `onRowSelectionChange` from its own state; without them the checkboxes do
  nothing. All items, Initiatives, and the Sessions table select; an enum column changes from a drop-up (`choices`).
- **Empty lists.** The kit has no empty-state slot, so render the empty message instead of the table. A list whose
  controls are in the titlebar keeps its table mounted with no rows, hidden behind the message, since the controls
  and filter tags live only while the table does (`emptyNotice` on `ActionItemTable` and `InitiativeTable`).

### `@jalapenolabs/cli/eslint`

`eslint.config.ts` extends the shared config. It adds three local blocks:

- **License header.** Headers must read `// Copyright © <year> Jalapeno Labs`. The shared config spells it
  `JalapenoLabs`.
- **Toasts.** `no-restricted-imports` refuses `toast` from `@heroui/react` outside `src/toast/toast.tsx`, so every
  toast keeps Elysium's timing and reaches the notification menu (see Toasts).
- **React rules.** The shared config at commit `e3f02f8` intends to apply `react/recommended` and
  `react-hooks/recommended` to TSX, but drops them. It passes `fixupConfigRules`' array result along as if it
  were a single config. The local block restores those rules, using the same plugin versions, until the shared
  package is fixed.
- **Formatting.** `new Intl.DateTimeFormat`, `Intl.NumberFormat`, `Intl.RelativeTimeFormat`, `Intl.DurationFormat`,
  and `toLocaleString`, `toLocaleDateString`, or `toLocaleTimeString` fail outside `src/format/` (Dates and
  numbers).

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
| `elysia`         | Elysia at a glance, her conversation oldest first, her turns newest first, and whether her panel is open and on what |
| `initiativeLinks` | Containers of the initiatives viewed, oldest first                     |
| `initiatives`    | Live and deleted initiatives in separate halves, by name                |
| `llms`           | LLM credentials, sorted by priority                                     |
| `mailAccounts`   | Connected mailboxes, sorted by address                                  |
| `mailDomains`    | The mail server's domains, sorted by name                               |
| `mailServer`     | The mail server's state, replaced whole by every update                 |
| `modelSettings`  | What each step and agent runs on, and the models and efforts offered; admins |
| `notifications` | Toasts kept for the notification menu, newest first, and which are read |
| `projects`       | Projects, sorted by name                                                |
| `githubCredentials` | GitHub tokens, sorted by name                                        |
| `jiraCredentials` | Jira Cloud credentials, sorted by name                                 |
| `learnings`      | Live and deleted learnings in separate halves, last changed first       |
| `companyDoc`     | The company doc, each section with its pending proposals, replaced whole |
| `environmentVariables` | Environment variables, sorted by key                              |
| `storageLocations` | Storage locations, sorted by name                                     |
| `studioItems`    | Live and deleted Studio items in separate halves, newest first          |
| `studioAssets`   | Every version of the files of the Studio items viewed, newest first     |
| `studioFeedback` | Drawn prompts of the Studio items viewed, oldest first                  |
| `codingSessions` | Coding and Studio sessions with what each is doing, newest first        |
| `drafts`         | The person's drafts of the records viewed, newest first                 |
| `sessionEvents`  | Events for conversations that are open, merged by sequence              |
| `sessionsView`   | Whether the Coding page's Sessions panel shows a table or tiles         |
| `sidebar`        | Whether the sidebar is collapsed to its icons, remembered per browser   |
| `realtime`       | Event stream connection: `connecting`, `open`, or `reconnecting`        |
| `theme`          | Theme preference and what it resolves to                                |
| `updates`        | Auto-update's status, loaded by admins and sent to everyone by `updates.status` |
| `version`        | The build this page loaded, the one the API runs now, and a dismissed refresh |

Server collections use entity adapters. Redux is the source of truth components render from.

### Server data: SWR, then Redux, then the event stream

1. **SWR loads it once.** A component that shows server data calls a loader from `src/hooks/useServerData.ts`
   (`useLlmsLoader`, `useMailAccountsLoader`, `useMailServerLoader`, `useMailDomainsLoader`, `useProjectsLoader`,
   `useStorageLocationsLoader`, `useGithubCredentialsLoader`, `useJiraCredentialsLoader`,
   `useEnvironmentVariablesLoader`, `useLearningsLoader`, `useDeletedLearningsLoader`, `useLearningLoader`,
   `useCompanyDocLoader`,
   `useCodingSessionsLoader`, `useModelSettingsLoader`,
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
- **First field.** Every page and dialog focuses the field typing starts in as it opens, with `useInitialFocus`
  (`src/hooks/useInitialFocus.ts`): a ref callback handed to the input, text area, or button itself, never the
  `autoFocus` attribute. It runs whenever its element attaches, so a dialog mounted once for the whole app, or a
  picker's search in a popover, focuses its field on every opening. It takes a condition for a form whose first
  field depends on what it starts from (a session from an item starts at its prompt).
- **Ids.** Every field has an id written by hand, `<form>-<field>` in kebab case (`action-item-title`), with the
  record's id in it where the field repeats (`reply-<recordId>-text`, a person's role on Users). React Aria's
  generated ids change from render to render, so they do not count. The shared fields (`SecretInput`,
  `OptionSelect`, `DayPicker`, `MultiPicker`, `ProjectScopePicker`, `GithubTokenSelect`, `ListSearchField`,
  `InlineEditableText`, `SetupChecklist`) require one; a picker's popover search is `<id>-search`.
- **Autofill.** Every text input sets `autoComplete`: the person's own details as what they are (`email`,
  `given-name`, `family-name`, `username webauthn`, `current-password`, `new-password`, `one-time-code`), `url`
  for an address they own, and `off` for everything else (names of records, keys, searches, codes).
  `src/bugs/167.test.tsx` checks the `/new` forms for all three.
- **Drafts.** What is typed and not yet saved or sent is kept in session storage for the tab, so a reload or a crash
  gives it back (`~/.claude/docs/ux.md`, Form inputs). `src/drafts/sessionDrafts.ts` reads, keeps, and forgets a
  draft under one name (`elysium.draft.v1.<name>`), in a try/catch, so a browser without storage still has a working
  form. A draft with nothing typed in it is never written, so blanking a box or a form opening blank never wipes
  what was kept; it ends only when forgotten.
  - **New forms.** `useFormDraft(form, { name, fields, choices })` keeps a react-hook-form form's listed text fields
    and the choices beside them, puts them back into the blank form as changes when it opens (so Save is ready and
    leaving asks first), and returns `forget`, which the form calls once it creates and registers as its page save's
    `onDiscard`, so Discard work ends the draft too. Every `/new` form has one: items, initiatives, projects, coding
    sessions (one per item a session starts from), Studio items, learnings and their categories, environment
    variables, storage locations, LLMs, the mail server, domains, and mailboxes, and the password vault. A token
    connector keeps its account's fields but the secret ones.
  - **Never secrets.** A form lists what it keeps, and a `SecretInput` field is never on the list; an environment
    variable's value is listed only while it is not secret (`fields` may be a function of the values). Fields a page
    presets from its address, such as `?project=`, and files are left out too.
  - **Boxes of text.** `useTextDraft(name)` is state for one box: the session and Studio prompts (named by the field's
    id), comments per item, replies per record, the report note per thing reported, and Elysia's message and
    answers. Sending forgets it.
  - `src/bugs/168.test.tsx` plays a reload, a create, Discard work, and a secret.
- Read field values with `useWatch`, not `form.watch`. The React Compiler lint rejects `watch`.
- HeroUI v3 field `onChange` handlers receive the value, not a DOM event.
- A `Switch`'s clickable part is `Switch.Content`, so it wraps the `Switch.Control` and the `Label` together. A
  `Description` sits beside `Switch.Content`, inside the `Switch`. A control outside `Switch.Content` looks right but
  does nothing when clicked; `src/bugs/switchClickTarget.test.tsx` fails the build on one.
- Server conflicts map back to fields. For example, a `409` on create or edit sets an error on `name`.
- Every secret field is `SecretInput` (`src/components/SecretInput.tsx`): hidden while typed, with an eye button
  to show it and an eye with a slash to hide it, and `autoComplete='off'` unless it is the account's own password
  (`current-password` or `new-password`). Tokens, keys, and secret values set `isMonospace`. Passwords, API tokens,
  storage keys, the vault's secret and master password, connector accounts' secrets, and secret environment values
  all use it.
- Secret tokens are write-only. The edit form leaves the field blank and sends a token only when one is typed, with
  the field's description saying a blank one keeps the stored token. This is a deliberate exception to
  `~/.claude/docs/ux.md`, which avoids "leave blank to keep" for passwords: an API token is replaced in its own
  provider and pasted here, never typed from memory or reset, so a separate replace dialog would only add a step.

## Overlays

A modal or alert dialog opened from code, not from a trigger child, skips the `Modal` or `AlertDialog` root. It
passes `isOpen` and `onOpenChange` to `Modal.Backdrop` or `AlertDialog.Backdrop`, fed by `useOverlayState`. The
root is a trigger wrapper, and rendering it without a pressable child makes React Aria warn.

## Toasts

Every toast goes through `toast` in `src/toast/toast.tsx`, never HeroUI's: ESLint refuses importing `toast` from
`@heroui/react` anywhere else. It keeps HeroUI's shape (`toast`, `toast.success`, `toast.warning`, `toast.danger`,
`toast.close`) with string messages, and adds:

- **Timing.** Warnings and errors stay until dismissed. Everything else stays 1 second plus 300ms per word of its
  message, description, and action, and at least 6 seconds when it has an action. The policy is `toastTimeoutMs` in
  `src/toast/toastPolicy.ts`, with its numbers in `src/constants.ts`.
- **Action.** `action: { label, onPress }` puts one button on the toast, such as Undo; pressing it closes the toast.
- **Link.** `link: { label, href }` puts a real link under the message, opening in a new tab, such as to an issue
  just filed. Descriptions are strings, so a toast stays serializable for the notification menu.
- **Work in progress.** `toast.loading(message)` shows a spinner until `toast.close` is called with its id.
- **Kept for the notification menu.** Every toast but a success and a loading one is recorded in
  `notificationsSlice` (title, description, link, variant, when, and whether it was read), newest first, at most 100.
  A success confirms what the person just did, so it is left out; `keepInNotifications` overrides either way. They are
  kept in memory for the page's life alone, like every other slice: a reload starts empty, and signing out, a full
  page load, leaves nothing for whoever signs in next.
- **The notification menu.** The topbar's bell carries a badge counting what is unread, and opens
  `NotificationDrawer` from the right: every kept toast, newest first, each with its status icon and an edge in its
  status color, its description and link, when it was shown, and a button to clear it; then Clear all and Close at
  the bottom. Whatever the open drawer shows is read, so opening it clears the badge, and anything arriving while it
  is open never counts.
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

Users choose Light, Dark, or System under Settings, My account. The choice is saved per browser in
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
  vanish against the page. Dividers (`--separator`) are a 16% tint of the muted text rather than a color, so one
  shows the same on the page, the side panels, and the menus and popovers, which sit lighter than the page in dark
  mode. HeroUI's soft accent is deepened in dark mode (a 30% tint, its text the link blue), since its own reads
  dimmer than a plain chip there and an accented count must stand out.

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

The same file points uikit's `--jala-table-*` variables at the palette through `html .jala-table-theme`, including
the dark table colors.

`src/theme/dockview.css` defines `dockview-theme-elysium`, which points Dockview's `--dv-*` variables at the same
tokens.

## Translations

Every user-facing string goes through i18next. `src/i18n.ts` bundles the locale files and initializes before the
first render.

- `en-US` is the source locale and the only one shipped today.
- Namespaces are one file each under `src/locales/en-US/`: `common`, `navigation`, `settings`, `llms`,
  `email`, `storage`, `github`, `jira`, `environment`, `learnings`, `companyDoc`, `projects`, `coding`,
  `actionItems`, `initiatives`, `changesets`, `studio`, `elysia`, `runs`, `search`, `models`.
- `src/@types/i18next.d.ts` types every key, so a missing or misspelled key fails `yarn typecheck`.
- Enum values such as LLM types and statuses are translated through lookup tables typed with
  `satisfies Record<..., ParseKeys<'llms'>>`.
- Logs and `console.debug` messages stay in English.
- **Copy** (`~/.claude/docs/ux.md`, Brevity, Positive wording, Dev babble). A page never opens with a paragraph about
  itself: what truly needs explaining goes behind `help`. A field's hint gives its format or an example, never what
  saving does next. A message that says what can't be done says what can, and nothing names Elysium's parts (Docker,
  nginx, ports outside the instructions that need them, configuration files, the OAuth broker). A destructive
  confirmation always keeps saying what can't be undone.

## Dates and numbers

The server sends and expects UTC ISO 8601 timestamps. Time zones exist only in the frontend: every date is shown in
the viewer's own zone.

Every date, time, duration, and number the app shows goes through `src/format/`. It builds each `Intl` formatter once
per language and options, in the language i18next renders in, and lint fails on formatting anywhere else.

| Function              | Reads                      | For                                                          |
|-----------------------|----------------------------|--------------------------------------------------------------|
| `formatDate`          | January 1st 2026           | A day in a sentence or a details panel                       |
| `formatDateTime`      | January 1st 2026 at 8:56pm | A moment in a sentence or a details panel                    |
| `formatShortDate`     | January 1st                | Tables and lists, through `DateText`; other years keep theirs |
| `formatShortDateTime` | January 1st at 8:56pm      | Tight places where the time matters: messages, activity      |
| `formatTime`          | 8:56pm                     | A moment within the hour, such as a link's expiry            |
| `formatCalendarDay`   | January 5th 2026           | A provider's `YYYY-MM-DD` day, read in UTC so it never shifts |
| `formatWeekday`       | Mon                        | The days of a weekly schedule                                |
| `formatRelativeTime`  | 5 minutes ago              | A moment relative to now                                     |
| `formatDuration`      | 850ms, 2.4s, 3m 4s, 2h 5m  | How long something took                                      |
| `formatMediaPosition` | 3:04, 1:02:03              | A point in a recording                                       |
| `formatNumber`        | 1.4M                       | Any count or quantity: one decimal, uppercase K, M, and B    |
| `formatPercent`       | 78.4%                      | A share from 0 to 1                                          |
| `formatBytes`         | 1.6 TB                     | A size, in decimal units, to one decimal                     |
| `formatMoney`         | $1,234.50                  | An amount of money, always to two decimals                   |

- Months are never numbers. `common:dates` holds the order of a date's parts, the day's ordinal suffix, and "at";
  the parts come from `Intl`'s `formatToParts`, never its punctuation, which changes between ICU versions.
- `DateText` shows the short date in a `<time>` with the full timestamp in a tooltip. Tables and lists use it. A day
  such as a due date passes `isDay`, so its tooltip leaves out the time. Its trigger is a tab stop, so the timestamp
  reaches the keyboard; tables pass `isFocusable={false}`, since a page of rows would otherwise put a tab stop on every
  date between the controls, and the tooltip still shows on hover.
- Translations compress counts with `{{count, compact}}`, registered in `src/i18n.ts`; `count` stays a number for
  choosing the plural. `src/format/translations.test.ts` fails on a `{{count}}` without it. Identifiers, such as a
  session's number, and exact limits stay plain.
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
- The knowledge base as a kind of search result, for admins and members, once a search reads its index.
- The theme preference moving from each browser to the person's profile.
