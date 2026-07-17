# Elysium

I have a vision, for an application that is open-source, self hosted, and is an entire business workspace all in one.

I'm fed up with Google Workspace and Microsoft Teams. One-drive is a bolt on, Google Drive is annoying and expensive. Cloud stuff kinda sucks, management UI kinda sucks, separate tooling implementation all sucks, and I wish it could have been done way better. The fact that I
can't self host also sucks.

What I'm thinking:
- Open source
- Self hostable on a machine
- SMTP server (a mail client)
- Team chat (a slack-like client)
- Storage service (a drive-like client with official S3 binding support) (can use local file storage or a cloud S3-like bucket)
- Perforce protocol server implementation
- Git protocol server implementation
- Team management
- Admin console
- Automation
- Tasking built in
- A claude agentic workspace

ALL in ONE.

AND I want it to either host it's own everything (postgres, storage to local file hard drive, etc) AND optionally connect to existing solutions. For example, I should be able to connect Google Drive to it and now all of my Google Drive files can sync with it. I could either
use Google Drive as the storage, or use my own. I could either use Slack for messaging or use my own. I could either use Github or roll my own. I could use Jira or roll my own tasking manager. I could link to Gmail and use that as the backbone, or roll my own.

Here's the major thing though-- I do NOT want to just re-create these tools 1:1. They all have major flaws and frustrations. The whole point of this is to challenge the UX, challenge the system, unify, unite. NOT to re-create.

This should be an AI-first tool. Capable of multi-tasking, working on multiple tasks/projects at once, reporting usage, saving memories, syncing project output, directly working with the git, perforce, and storage protocols on disk. AND Claude agentic workers should be deployable as containers onto remote machines as docker containers for computing purposes, like registered workers.

The workspace itself will be dramatically different than most.
I want to challenge the UX, push UI to it's limits, really change and flip the model and paradym of how we've used these tools!!

First off, the main screen on the UI will be a Claude session. It will have the Claude chat in it, and lots of tooling + information to report more about it.
That being said, it WONT be a traditional message system. It will be talking to Claude Code but not just "my messages" and "your messages" it will be a full suite of a code session.
Next, it will have dramatically a different background (futuristic, artistic, sci-fi) which describes what it's doing.
Orange when active/running, red when errored, pink when it needs help or user input, and green when it's done.
Then, it'll be on a icosahedron-like interface. The user can switch between Claude Code sessions as it rotates.
There will be a voice integration, where the agent reports things as it goes and reports things out like to the user.
There will be sound effects (agent completion, needs help, error, etc)
There will be LOTS of settings- Both user and team and org wide.
This is literally the first and primary view, and the most important. It will be dynamic, fluid, interesting to look at, with lots more to add onto it in the future.

On the sidebar will be the primary user actionables. Things like git, perforce, drive, messages, etc.

As I click them, by default it'll open a side-by-side layout where on the right is the primary Claude interface and on the left is the new panel of actionables that they opened.
So I can open the drive on the left, and keep Claude on the right.

This layout should be DYNAMIC!! I should be able to pop it out, drag it, re-panel them, etc. We'll use rc-dock as the primary mechanism to do so.

I should be able to deploy this on a computer I have at home, expose it to the internet (SSL and hosting are the user's responsibility-- Not ours), and anyone should who can access it (who is authorized) should be able to use it, and see/manage the files on the system, etc.

Users can belong to teams and users and teams have individual permissions.
Examples of teams are admin teams, dev teams, qa teams, contractor teams, intern teams, etc.

Identity is multi-org and multi-account. A profile is a deployment-global identity
keyed by a globally-unique email and can belong to many organizations at once (one
deployment can host several orgs, e.g. NavarroCity + JalapenoLabs + MooresLabAI). A
person can hold several distinct profiles and be signed into more than one at the
same time, swapping between them (a profile switcher) and between orgs (an org
switcher), Google-Workspace style. Email is the primary source of auth truth.
Eventually orgs will own email domains and be able to issue addresses under them
(alex@navarrocity.com created by the NavarroCity org), which may use a different
initial-authentication path; this rides on the planned SMTP integration and does not
change email being the global profile key. See docs/identity.md for the built model.

On the Claude level, I should be able to OAuth to multiple Claude subscriptions and then if my usage dries up one one of them with the subscription it'll auto-switch to the next one.

# Challenging the system

We will NOT re-inventing existing email and messaging UI-- But instead we'll be combining it and making it a totally different experience.
Note: I'm always open to ideas, feel free to suggest ideas to me at any time.

Instead of having emails separate from a messaging system... Why not combine them?
Imagine if Microsoft Teams would show you emails in the sidebar like it was a conversation? Then you could click on it, take actions, and manage it.

Also, most of the time, I perform an action (respond to someone, respond to an email, etc) and then I don't need to reference it again.
It doesn't make sense to me, when I go into Slack and see a ton of empty pinned conversations and channels. It matters if I need to send, but not when I need to receive?
Then Slack/Teams chimes, I open the tab, and have to scroll around to find it. We're also doing away with that.

When messages are received, we take the user to it. The UI is going to be very heavily focused on "we bring things TO you, the user" not "you the user are given everything and need to hunt things down."
This includes that when Claude outputs a file onto drive for review, the user can follow it and see it in their workspace immediately. It's all about displaying relevant information in real time.

The user isn't so much driving, the user is the passenger and Claude + automation is driving. The user as the passenger is given what they need to make quick and informed decisions, and can let the system operate.

The system itself is autonomous, like it's own standalone developer / team member. It can manage files, perform actions, write docs, submit proposals, and more.

I even want a built in browser tab, so Claude can pull up a browser tab, scroll to things, take screenshots, highlight text, etc.
If the user says "I want to order a pizza nearby" it shows a map on the left panel with x3 locations, the user picks A/B/C and then it'll run the Chrome website, show the user pricing info, and the user just sits back and watches it happen.
If the user says "Research and show me my YouTube competitors" then Claude can make a spreadsheet onto the drive, do the research that it needs, and then open x3 main tabs of each competitor's YouTube channels inside the workspace.

So I want to challenge the system! First, have a flagship "action items" style of messaging center.
As a user, you no longer open up a chat channel tab (Teams/Slack/Discord), and you no longer open an email tab (Outlook/Gmail/etc), and you no longer open a drive tab (Onedrive/GoogleDrive), and you no longer open several git tabs and etc etc... Instead, everything is brought to you in your action center. You can navigate quickly, anywhere with ctrl+p (vs-code style) and look up people, contacts, emails, messages, github repos, etc. Lightning fast navigation, universal searchbar, the whole nine yards.

We can even leverage AI to auto-draft emails/messages (without the user even asking, it just presents options).
I also think that Claude clients can retain memories and contexts, so when a project manager asks for an update about a feature the Claude agent can remember this as part of the draft. It can look at email threads and other history too!

There will constantly be a fine line between "when do we redirect the user automatically" vs "the user should redirect themselves because they're using it." For example, if I'm in the middle of reading something then I may not want to be auto redirected. I think we can have an auto-pause system and can auto-resume or "go to next action" that kicks things off again. As soon as I mark an email as resolved (note: I didn't say "read" I said resolved intentionally) or as soon as I respond to it, it can move me onto the next action automatically.

As an organization, I no longer need all of these separate toolings and controls. It's all in one.

Errors (Claude errors and client errors and etc) should be in a universal location, like a "internal app notifications" center. That way, issues can be audited and fixed pretty quickly.

Admins can have visibility into their user's claude accounts, conversations, usage, etc.

We'll support all kinds of features:
- User profile avatars
- User bios and information
- User status (online/offline/last-seen/ooo)

# Desired architecture

- Rust backend API
- Rust deployable workers with their own API (these run in docker containers on separate machines)
- Protobuf, everywhere (between frontend and API workers) (frontend - well typed, generated at build time NOT runtime)
- Postgres (latest but a pinned version of the latest)
- RabbitMQ + amqp between the workers and the API
- Full typescript
- React latest + HeroUI v2 + TailwindCSS + Vitest + Redux
- Electron
- Multi-window / Multi-tab
- Worker containers will leverage `claude -p`
- Organizations can provide oragnization-wide Claude subscriptions, or teams can bring their own or users can each bring their own.
- Perforce integration will use JalapenoLab's Astraval technology built-in, so the user can leverage all of perforce's protocol with none of the cost

In production we'll deploy as an Electron GUI.
The user should be able to open multiple windows and manage the rc-dock between them.
This may involve syncing redux between windows, to share the state between them (I recommend using a shared web worker to do so).
Maybe I want my action center as an RC dock panel on the left monitor, in it's own window, and I want the main interface on the primary monitor.
In the GUI, this is totally possible. In a browser, it's no problem to limit the user to a single tab.

It should also be able to be used as a standalone in-browser app. We should use preload as little as possible, and put the minimal amount of things into electron.

# Storage and service integrations

Two foundational pillars, fully designed in `docs/storage.md` and
`docs/integrations.md`. Those docs are the source of truth; the summary here is the
load-bearing shape.

- **Storage is a gateway, not a sync engine.** An org owns many **Drives** (local FS,
  S3, SFTP in v1; WebDAV/OAuth clouds later), each an OpenDAL operator rooted where the
  org points it. The provider is always the source of truth; nothing is mirrored. Search
  is live, federated, streaming, and index-free. Transfer proxies through the backend
  (presign-if-available later). Access is Drive-level (org-wide vs restricted, admin
  bypass); per-file Google-Drive-style sharing is a deferred pillar.

- **Providers advertise capabilities; consumers require them.** `PLAIN_FILES` (any
  provider), `OBJECT_STORE` (FS + S3), `POSIX_FS` (local FS only). A consumer binds each
  storage role to a capability-eligible Drive. This is why git and Astraval-metadata
  (POSIX_FS) are local-FS-only, and why "git on OneDrive" is an enforced impossibility.

- **Elysium is a control plane, not a re-implementation.** We do not rebuild git, SMTP, a
  registry, or Perforce. Elysium **provisions and manages best-of-breed OSS containers**
  (Forgejo for git, a mail server, a registry, Astraval) and unifies them. Provisioning is
  behind a **`Provisioner` abstraction**, Docker-first (Podman-friendly, behind a scoped
  socket-proxy, never the raw socket on the API), with Kubernetes as a later opt-in
  backend. Managed integrations are desired state in Postgres, reconciled on boot.

- **An integration is to services what a Drive is to files.** Many per org, each a `kind`
  (git/mail/registry/perforce/runner), a `mode` (managed | linked), and an `owner_scope`
  (org | personal). Unified surfaces aggregate across instances ("all my repos", "all my
  mail"). Provisioning is admin-only (`manage_integrations`); personal linking is gated by
  per-kind admin policies.

- **One login, zero re-prompt, for the user and Elysia.** Elysium is the single identity
  authority, extended to be an **OIDC/OAuth2 provider** so managed services SSO against it
  (plus reverse-proxy forward-auth for zero-flicker), and to **mint SSH keys + scoped
  tokens** for git/registry/SMTP transport auth. For linked services Elysium is the OAuth
  *client*, silently refreshing the user's grant. The token-minting layer serves the AI
  exactly as it serves the user.

# Frontend tooling and shared Jalapeno Labs packages

These are Jalapeno Labs house standards. They are installed straight from their
public GitHub repos (not published to npm yet), so both must be allowlisted in
`frontend/.yarnrc.yml` under `approvedGitRepositories`, with `enableScripts: true`.

- **ESLint: `@jalapenolabs/cli`.** The frontend lint config is `eslint.config.ts`,
  which extends `@jalapenolabs/cli/eslint` (the shared Google/Jalapeno TypeScript
  style: no semicolons, single quotes, stroustrup braces, `object-curly-spacing`,
  no default exports, license headers). We override the license header to
  `// Copyright © <year> Jalapeno Labs` and ignore generated proto output. Run
  `yarn lint` / `yarn lint:fix`. Do not hand-roll ESLint rules; changes to the
  shared style belong in the CLI repo.

- **Shared UI: `@jalapenolabs/uikit`.** The reusable React component library,
  hooks, utilities, and brand tokens. Consumed straight from git, pinned to the
  `develop` branch (`frontend/package.json`:
  `"@jalapenolabs/uikit": "https://github.com/JalapenoLabs/uikit.git#develop"`),
  the same way `@jalapenolabs/cli` is. It builds on install via its `prepare`
  script and needs no HeroUI/Tailwind coupling (React 18+ peer only). **Prefer its
  components, hooks, and utils over rebuilding primitives, and contribute sharable
  UI back to `uikit` rather than duplicating it in Elysium.** It ships components
  (Button, Accordion, Loader, Timer, DisabledTimerButton, LanguageSelector,
  ThemeSelector, StatusChip, DragNDrop, LuminescentText, ...), hooks (`useHotkey`,
  `usePagination`, `useScrollOnMount`, `useScrollPositionRestore`, ...), and utils
  (`parseJsonSafe`, `reorderArray`, `formatBytes`, `fuzzyMatch`, `deepDiff`,
  `clipText`, ...). Because these are git dependencies, the frontend Docker image
  installs `git` + `ca-certificates` so a clean build can fetch them.

# Version pinning

Pin tool versions everywhere; never rely on a floating `latest` tag. Yarn is
pinned to `4.17.0` (via `packageManager` and an explicit `corepack prepare` in
CI). Node, Rust, and Postgres are all pinned to exact versions in CI and Docker
so local, CI, and production never drift.

# Naming

The application and deployable will be called Elysium.
The AI within will be called/named Elysia.
This is hosted, maintained and created by JalapenoLabs LLC

# CI monitoring

After every git push, monitor the resulting CI run in the background and keep working;
only block on the run when the push is the end of your task.
When working through multiple commits in a row, fold any CI failures into the next wave
of commits instead of stopping to fix each one. Iterate until CI is green.

# Unit testing

Write unit tests for everything you write on both the frontend and the backend.
The only exception is page-level JSX (page components); everything else gets tests.
Feel free to use a sub-agent to write unit tests, to help preserve context.

# Claude.md

The USER is the source of truth, and this CLAUDE.md document is the second source of truth.
The code is NOT the source of truth, as it could change/evolve, or introduce tech debt.
You are to keep this doc up to date as the user makes decisions.
If the code drifts from this doc, report it to the user.

# Documentation

There should be a @docs/ directory in this repository.
Inside of docs, maintain a .md document for each category of decisions made for the application.
For example, docs/email.md would be a good place to keep up with and learn about existing email decisions.
Keep it uniform, follow the docs and leverage the documentation in docs. Keep it up to date as you go! (critical)

## History

We do NOT document history, we document current implmentation and future plans only.

Good examples:
We can say "Today the system does X" or "the roadmap is to do X/Y/Z"  or "Z is remaining on the roadmap"

but we do NOT include details such as "we migrated from __" or "we used to do __" or "we implemented X/Y but not Z on the roadmap"
