# Vision

Elysium is a personal orchestrator: one deployment per life, one for work and one for personal, each running on a
host that is always on, so the user's own computers are clients of it rather than the machine doing the work. Elysia
is the assistant inside it. The aim is that the user attends meetings, reads, and decides, and Elysia does the rest.

This doc is the whole picture. Each area's implementation lives in its own doc; this one says where they are going.

## The problem

Claude Code is capable but reactive. It sits idle until it is fed, runs for a while, and goes idle again. The user
ends up as its dispatcher: queueing tasks, relaying everything said in a meeting, creating and assigning issues,
watching PRs, and nudging it to keep going. Elysium closes that gap. Work arrives from the world, not from prompts.

## Elysia starts the work

- Events create work: mail, chat messages, meetings, and changes in providers such as Jira and GitHub.
- Elysia reads each one, attributes it to projects, records it, and decides whether it is hers to do, the user's
  to decide, or information only.
- When it is hers, she starts on it without being asked and says so: "I'm already working on that deck; you can
  review it in 30 minutes."
- When she is blocked she asks one clear question with suggested answers, and continues when it is answered.
- She keeps going: when nothing is queued she revisits what she gave up on, and a turn that dies is detected,
  recorded, and revived.

Seraphim proved this loop for issues (take the next work when idle, fix failing CI, address review comments,
revive dead turns, ask when blocked, pause on usage limits, keep an availability schedule, suggest follow-up work).
Elysium carries those ideas forward redesigned around action items and projects, not around a kanban board.

Coding itself, opening PRs, and watching CI are what a Claude Code session already does well. Elysium does not
reimplement them; it decides when they should happen and follows the result.

## Autonomy is earned per kind of action

Nothing Elysia does needs the user's approval because of who proposed it; what matters is how reversible it is.

| Tier             | Examples                                                         |
|------------------|------------------------------------------------------------------|
| Done             | Drafting, labelling, marking spam or read, filing, opening a PR  |
| Done and shown   | Archiving, moving, creating tasks from what was said             |
| Waits for you    | Anything sent to a person, payments, deleting                    |

Every kind starts conservative and moves up as Elysia's track record earns it. Changesets are how the "waits for
you" tier is reviewed. Which kinds start in which tier is not decided yet.

## Elysium acts as its user

Elysium uses services the way the user does, signed in as the user, and never installs anything into an
organization. Mail sent from Elysium is sent from the user's own account in the user's own voice, with nothing that
marks it as written by an AI.

Each service is a **connector** behind one trait, with four parts:

| Part           | Ways to fill it                                                                             |
|----------------|---------------------------------------------------------------------------------------------|
| Sign in        | A token the user pastes (GitHub, Jira), an official OAuth app, or a browser session         |
| Stay signed in | Refreshing a token, or keeping the browser profile running                                  |
| Ingest         | An API watch, IMAP `IDLE`, or a cheap check inside the signed-in page                       |
| Act            | An API, a CLI running as the user (such as `gh`), or Elysia driving the page                 |

Spotting new activity is deterministic code that costs nothing to run often. Reading and acting is Elysia's turn,
which reads a page the way a person does, so a web app's redesign does not break her the way it breaks a scraper.

### Remote browser

Signing in to Outlook, Gmail, and anything else without an app of Elysium's own happens in a real browser that the
host runs and streams to the user over WebRTC. The user signs in on the real page, including OAuth, MFA, and
passkeys; credentials never pass through Elysium's code. The browser profile stays running on the host, which keeps
the session alive.

- One browser container and one profile volume per signed-in account. A profile is a secret: its cookies are the
  user's sessions.
- The stream is a privileged, authenticated, same-origin route, proxied by nginx, so the Content Security Policy
  holds.
- Elysia drives the same browser the user signed in to, so the user can watch her work live in a panel: ordering a
  pizza, filling a form, researching.
- Off-the-shelf streaming (Neko) is preferred over building one. Elysium provisions it through `docker-proxy`, the
  way it provisions Stalwart.

A provider that refuses sign-ins from a datacenter address is handled when it is met; routing the browser through a
VPN is the likely answer.

### Connections on the roadmap

- Mail: Outlook and Gmail first, through the remote browser.
- Chat: Slack and Teams, read and drafted as the user.
- Meetings: recordings and transcripts from Meet, Teams, Zoom, and note takers such as Fireflies.
- Passwords: Bitwarden through its CLI, limited to a collection reserved for Elysia, so she can sign in to things on
  her own. Never the whole vault.
- Code: every repository the user works in (twenty and more), through `gh` and Jira as the user.
- The desktop: a full desktop on the host that Elysia drives with mouse, keyboard, and screen, for what no web page
  or API covers.

## One timeline per project

Projects never end; initiatives are goals inside them that do. Everything ingested is one kind of record (source,
time, who, content, and optionally a span inside a recording) and lands on one timeline, attributed to projects by
Elysia with a confidence the user can correct.

Opening a project shows everything about it in order: every mail, chat message, note, agent turn, issue change, and
meeting segment. When a two hour meeting spent five minutes on a project, the project shows that five minute
segment: the recording trimmed to it, its transcript, and its summary. Segments are cut by topic from the
timestamped transcript, trimmed with `ffmpeg`, and stored in the project's storage location.

When a manager asks in chat for a change to a product, Elysia records it on that product's project, makes the item,
and does the work when she can.

## Agent logs read like GitHub Actions

Every turn of every agent is kept in full. Each turn collapses to a one or two sentence summary, written by a small
model when it ends, which expands to its complete log. History is navigated by summaries; detail is one click away.

## The interface brings things to the user

The user is the passenger; Elysia and automation drive. The UI exists to put what the user needs in front of them
so they can decide quickly. The current UI, modeled on Stripe's dashboard, is the base.

- Things come to the user, rather than the user hunting through mail, chat, drive, and repository tabs. Mail and
  chat are one stream of action items.
- The unit is resolved, not read. Resolving an item moves the user to the next one, with an automatic pause so the
  user is never pulled out of something mid-read.
- `ctrl+P` navigates to anything: people, mail, messages, projects, repositories.
- Elysia's status is visible at a glance by color: orange running, red errored, pink needs you, green done, with a
  sound for each and spoken updates as she goes.
- A dockable workspace: open a panel beside Elysia, split, rearrange, and pop out.
- Every failure, agent or application, lands in one fault center with a stable code. Nothing fails silently.

## Deployments

- Elysium is a personal Jalapeno Labs project. A workplace runs its published releases, updated daily, as a user,
  and never contributes to it.
- Work and personal are separate deployments that share nothing, each with its own Claude subscriptions.
- The host is always on: a dedicated VM. Remote compute for agents (Arsox satellites) is optional; agents run on
  the host by default.
- Claude is the agent. Codex is used for occasional automated tasks only.

## Longer horizon

The original ambition is a self-hosted workspace that can replace hosted suites, each part either Elysium's own or a
link to what the user already has: mail server (Stalwart, built), storage (built), and on the roadmap team chat,
git hosting (Forgejo), and Perforce (Astraval), all behind one identity. These follow the orchestrator, not the
other way around.
