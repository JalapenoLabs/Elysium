# Coding

The Coding area runs agent sessions on Elysium's own host: the official Claude Code CLI, in a workspace container
the engine owns (`docs/engine.md`). Sessions are started and followed on the Coding page; Studio items (`docs/studio.md`) are sessions too.

## The runner

`api/src/runner/` runs agent turns on Elysium's own host: the official `claude` CLI, headless
(`claude -p --output-format stream-json --verbose`), in the agent workspace container. It runs in the engine, and
everything that submits turns to it does too: sessions' turn queue, Elysia's dispatcher, the Project Managers'
dispatcher, turn summaries, meetings' segmentation, and the knowledge base's answers (`runner.submit(TurnRequest)`, or `submit_as` for a turn that already
has an id). Each reads the
turn's events from the ticket it returns. Only the CLI itself runs turns, on a subscription token or an API key; the
Agent SDK is never used with subscription credentials.

### Where turns run, and why there

Turns never run inside the API's or the engine's container: both hold `ELYSIUM_ENCRYPTION_KEY` and the database's
credentials, and an agent with every permission could read them. They run in one long-lived sibling container,
`elysium-workspace`, that the engine creates through `docker-proxy` like Stalwart, from the `elysium-workspace` image
of its own release (`workspace/Dockerfile`). Only the engine creates or replaces it; the API works in it once it
runs, for a session's files, a new session's clones, and Claude sign-ins, and answers `503` while it does not. A
sibling container keeps the API image small and lets the agents' toolchain change without touching the API. Per-turn
containers were not chosen: a turn would pay a container start, and work that spans turns (clones, build caches,
Claude's own transcripts) would need its own volume per conversation.

The worker pool's workers are the exception, by design: each runs its batch in a container of its own from the same
image, `elysium-worker-<batch id>`, with its own volume, network, and CPU limit, because a batch runs for hours or days
of its own turns, builds heavily, and holds its project's GitHub token (`docs/engine.md`, Workers). Its turns run on a
runner of their own in that container, exactly as here (the turn script, credentials, `~/.claude` fetched into its own
volume, a Blender and a desktop each turn), and never take one of the shared workspace's eight turn slots.

| Path                    | Holds                                                                         |
|-------------------------|-------------------------------------------------------------------------------|
| `/workspace`            | The volume `elysium-workspace`: everything that outlives the container        |
| `/workspace/.claude`    | `CLAUDE_CONFIG_DIR`, and the agent's `~/.claude`: settings, skills, transcripts |
| `/workspace/.elysium/turns/<turn>-<attempt>` | A running attempt's process group id and MCP configuration, removed as it ends |

Every turn is a `docker exec` as `agent` (uid 1001); Claude Code refuses to bypass permissions as root. The container
joins `elysium-agents` with the API alone, so agents reach Elysium's own MCP routes at `http://api:8080`
(`runner::AGENT_API_URL`) and the internet for git, packages, and the model. The image pins Claude Code and `gh`,
turns the CLI's self-updater off, and builds on the web app's Node. A container running another image is replaced at
startup, keeping its volume.

### `~/.claude`

Admins name a configuration repository under the workspace settings (`agentConfigurationRepository`, an https URL
without credentials). The runner fetches its default branch into `/workspace/.claude` before the first turn and again
whenever the setting changes, checking it out over the directory: tracked files are replaced and untracked ones,
including Claude's transcripts, are kept, so `--resume` survives a fetch. A github.com repository is read with the
workspace's default GitHub token, passed to git as an `http.extraHeader` in the environment and never written to
disk. A fetch that fails fails the turns that need it, with git's message.

### A turn

1. **Queue.** Turns of one conversation (`ConversationId`, chosen by the caller) run one after another; different
   conversations run side by side, at most `MAX_CONCURRENT_TURNS` (8) at once.
2. **Prepare.** The first turn after the engine starts (the engine also starts this at boot) ensures the container,
   kills turns a previous run left behind, and fetches `~/.claude`. Changing the configuration repository signals the
   engine to fetch it again.
3. **Attempt.** One `claude` process on one credential, with `--model` and `--effort` when the request names them.
   Every agent's turn names both, as resolved for it (`docs/engine.md`, Models), so they override whatever the
   agents' configuration repository's settings name. Secrets travel only in the exec's environment: the
   credential as `CLAUDE_CODE_OAUTH_TOKEN` or `ANTHROPIC_API_KEY`, and the MCP configuration, which the turn script
   writes to a private file in the turn's directory. `claude` runs in its own process group, so stopping it stops
   its tools too.
4. **Events.** Each line of `stream-json` becomes a `RunnerEvent` (`api/src/runner/events.rs`): `turnStarted`,
   `agentMessage`, `agentThinking`, `toolStarted`, `toolCompleted`, `rateLimit`, `credentialRolledOver`, and
   `other` for any line it does not understand, which is kept rather than dropped. Claude's other `system` lines are
   progress telemetry and are dropped.
5. **Finish.** Every turn's events end with one `finished`, carrying its status (`completed`, `failed`, `cancelled`,
   `interrupted`, `usage-limited`), Claude's final text, the error, the harness session id, and the cost in
   millionths of a dollar, never a float. The caller keeps the session id and passes it as the next turn's `resume`.

The runner keeps nothing: callers record the events they show.

A turn runs in one of three modes (`TurnMode`):

| Mode            | What it is                                                                                      | Used by |
|-----------------|-------------------------------------------------------------------------------------------------|---------|
| `Agent`         | Every built-in tool, the workspace's `~/.claude`, and a session the next turn resumes            | Coding and Studio sessions, Elysia |
| `Text`          | No built-in tools (`--tools=`), only the request's MCP servers (`--strict-mcp-config`), no settings (`--setting-sources=`), its own system prompt, and no session kept (`--no-session-persistence`) | Turn summaries, correction, segmenting, the company doc's review |
| `ResumableText` | As `Text`, with its session kept for the next turn to resume; Claude Code finds a session by its working directory, so every turn of one runs in the same one | The agent handling a conversation (`docs/engine.md`, Conversations) |

### Credentials

A turn runs on the active Claude credentials in priority order (`claude-code-oauth` as a subscription token,
`claude-account` as its current access token, `claude-api-token` as an API key); ChatGPT credentials, inactive, and
expired ones are skipped. A `claude-account` is also skipped while its sign-in is expired or its access token has
lapsed, and while its stored usage shows a window at or past 99% (`SKIP_AT_UTILIZATION` in
`api/src/claude_accounts/usage.rs`), until that window resets. When every credential that could serve is an account
held back that way, the turn finishes `usage-limited` at once, with `usageResetsAt` naming the earliest reset, rather
than running on the workspace's own login. An attempt whose
credential is spent (a `rejected` rate limit, or a result saying the limit is reached or the credit is gone) or
refused (an invalid key, an expired token, not logged in) hands the turn to the next credential, reported as
`credentialRolledOver`. If the attempt had begun a Claude session, the next one resumes it with a prompt to continue
where it stopped, so no work is lost; otherwise it runs the original prompt. Any other failure ends the turn at
once, since another credential would fail the same way. When every credential is spent the turn finishes
`usage-limited`, still carrying the session id to resume later. The CLI holds the credential itself, so an agent can
read it from its environment.

With no usable credential a turn still runs once, on whatever login the workspace holds, and normally fails with the
CLI's "Not logged in". The New session page warns before that happens and links to Settings, LLMs; its check
(`hasAgentCredential` in `frontend/src/pages/Settings/Llms/llmPresentation.ts`) mirrors the runner's.

### Claude accounts

A Claude account is signed in through the official CLI and kept alive by Elysium (`api/src/claude_accounts/`), the
rules ported from the `~/.claude/accounts` desktop switcher.

- **Signing in.** `claude auth login` runs as root in the workspace container, in a directory only root can read
  (`/root/.elysium/sign-ins/<id>`) with its own throwaway `CLAUDE_CONFIG_DIR`, so no agent can see a login in
  progress. With no terminal the CLI prints a manual sign-in link whose redirect is Anthropic's own code page, then
  waits at `Paste code here if prompted >`. It reads a FIFO opened read-write, so `POST
  /llms/claude-sign-ins` answers the link and `POST /llms/claude-sign-ins/{id}` writes the pasted code into the
  FIFO from a separate exec, as an environment variable rather than an argument. Elysium then reads the
  `.credentials.json` the CLI wrote, asks Anthropic's profile endpoint whose account it is, seals the tokens, and
  removes the directory. A wrong code ends the sign-in; the CLI exits on any answer. A sign-in waits at most 15
  minutes, and at most eight wait at once. What is waiting is kept in the API process that started it, which is one of
  the things that keep the API to one replica (`docs/engine.md`).
- **One refresher.** A turn is handed only the access token, as `CLAUDE_CODE_OAUTH_TOKEN`, never the refresh token:
  Claude Code rotates the refresh token on every refresh, so a second refresher would strand the first. The keeper
  (`keeper.rs`) runs every five minutes, after every turn, and after a sign-in. It renews an access token once
  fewer than three hours remain, so every turn starts with at least that long, and renews regardless once the refresh
  token is within seven days of expiring. Each refresh holds the account's row lock from reading the old refresh
  token until the new pair is stored, so two passes never send the same token. A refreshed access token does not cut
  a running turn short: the one it was handed stays valid until its own expiry.
- **A refused sign-in.** Anthropic answering `400` or `401` marks the account "Sign-in expired" once, announced on the
  event stream; the keeper stops trying it until a new sign-in replaces the token. Signing the same account in again
  keeps its credential, name, and priority. Any other failure is recorded on the account and retried next pass.
- **Usage.** Each pass reads every signed-in account's five-hour and seven-day windows from Anthropic's usage
  endpoint and stores them, announcing a change as `llm.usage`. Elysia's usage hold prefers the reset Elysium
  recorded (`usageResetsAt`) over one Claude reported mid-turn.

### Liveness

| Policy            | Value      | Why                                                           |
|-------------------|------------|---------------------------------------------------------------|
| Heartbeat         | 20 minutes | A model call or a quiet build takes minutes; twenty without a line is a hang |
| Concurrent turns  | 8          | Each is a `claude` process with its own tools on a shared host |
| Shutdown grace    | 15 seconds | Turns are killed and report `interrupted` before the engine exits |

A turn that prints nothing for the heartbeat is killed and finishes `interrupted`. A cancelled turn
(`runner.cancel(turn_id)`) is killed and finishes `cancelled`; one still queued finishes without running. Docker
calls for the runner allow 24 hours, because a turn's output is one exec stream held open for the whole turn.

### Testing

The runner's rules are tested against a scripted workspace (`runner/turn.rs`). One opt-in test drives a real daemon:
with `RUNNER_TEST_DOCKER_URL` naming a `docker-socket-proxy` with `EXEC=1`, `RUNNER_TEST_WORKSPACE_IMAGE` an image
such as `elysium-workspace:dev`, and an `elysium-agents` network, it runs a turn without a credential and expects the
CLI to report that it is not logged in. It creates the real `elysium-workspace` container and volume.

## Sessions on the runner

`api/src/sessions/` is everything between a session's row and the runner. A session is three things:

- **A directory**, `/workspace/sessions/<id>`, that every turn works in. Its repositories are cloned into
  `repos/<name>` when the session is created, Studio drawings land in `feedback/`, and whatever the agent leaves there
  stays until the session is deleted.
- **A Claude session**, `harness_session_id` on the row: the session id the last turn reported, resumed by the next
  turn with `--resume`. Claude keeps the transcript in `/workspace/.claude`, so a session carries on across restarts
  of the API, the engine, and the workspace.
- **Its events**, recorded in `session_events` (see History).

Each session is its own conversation on the runner, so its turns run one after another and never two at once.

### A turn

`POST /api/v1/coding-sessions/{id}/turns` queues a turn as a `session_turns` row, signals the engine, and answers at
once. The engine's turn queue (`api/src/sessions/queue.rs`) takes each session's oldest queued turn once the session
has none running, so each turn resumes the Claude session the one before it reported, and builds the turn's request
then. A turn queued while the engine is down waits for it. The turn carries:

- **The environment:** every workspace environment variable, then the session's GitHub token as `GH_TOKEN` with the
  git config that routes git through `gh` (`docs/environment.md`, `docs/github.md`).
- **Elysium's tools**, under a grant for this turn alone (see below).
- **A Blender of its own**, the `blender` MCP server (see below).
- **Studio's instructions** as `--append-system-prompt`, for a Studio session.

As the turn runs, every event is scrubbed of the turn's secret values (the GitHub token, every secret environment
variable, and every password and code the password vault handed to any agent, each of six characters or more, become
`[secret]`), given the session's next sequence, recorded, and only then
published as `session.event`. A client that refetches history after a live event therefore always finds it. When
the turn ends, the Claude session it reported is kept on the row, a Studio session's deliverables are collected, and
the grant is revoked.

What a session is doing (`idle`, `queued`, or `running`, with its queue depth and current turn) is read from its
turns' rows, so every process shows the same, and published as `session.upserted` whenever it changes. A turn is
`running` once its CLI started; one the engine took that waits for a slot still counts as queued. Stopping a session
(a Studio item hidden, a session deleted) finishes its queued turns unrun and marks the running one, which the engine
cancels. An engine that starts finishes a turn its predecessor's stop cut short, whose history then ends where it
stopped, and queues again one that was only waiting for a slot.

### Elysium's tools

Some of an agent's tools answer from Elysium's own data: `elysium_work` for the project's action items and
initiatives (`docs/action-items.md`), given to every turn of a session with a project, `elysium_storage` for its
storage locations (`docs/storage.md`), given when the project may use one, and `elysium_passwords` for the logins
agents sign in with (`docs/passwords.md`), given to every turn while the password vault is ready, and
`elysium_knowledge` for the company's knowledge base, given to every turn: searched with `knowledge_search`, opened
with `knowledge_get`, its meetings listed with `meetings_list`, and the company doc read with `company_doc_read` and
proposed to with `company_doc_propose` as the session's coding agent (`docs/knowledge.md`, MCP and The company doc).
Its instructions are the company's agent guidance, read as each turn connects, so an admin's edit reaches every turn
as it reaches every MCP client (`docs/knowledge.md`, Agent guidance). Its every call reads for whoever started the
session, checked in the database, so a session a guest started reaches none of it, and a session's searches spend its
starter's search budget. Another server,
`elysia`, is Elysia's own and is given only to her turns (`docs/elysia.md`), and `elysium_project_manager` is a
Project Manager's own, given with `elysium_work` and `elysium_knowledge` to its turns alone, which work for its project
and read as its machine user (`docs/engine.md`, Project Managers). They are served at
`/api/agent-tools/{server}` (`api/src/tools/endpoint.rs`), one stateless Streamable HTTP MCP server each, which the
workspace reaches as `http://api:8080` on `elysium-agents`. nginx answers `404` for the path, so it is never on the
public origin.

Before each turn the engine mints a grant (`api/src/tools/grants.rs`): a 256-bit bearer token naming the session's
scope and the servers the turn may use, stored in `agent_tool_grants`, since the API checks it. The token travels in
the turn's MCP configuration; only its SHA-256 is stored. A request without a live grant answers `401`, and one for a
server the grant does not name `404`. Every call is then checked against the database as before: the session's
project, its action item, its storage locations. The grant is revoked when the turn ends. The API answers these calls,
so while it is down an agent's calls to Elysium's tools fail and its turn carries on without them.

Every grant is leased, the same for sessions', Elysia's, and Project Managers' turns. While a turn holds its grant, a
task of the grant's own renews it every 15 seconds for another 60 (`expires_at`, set and compared on Postgres's clock,
so the engine that writes it and the API that reads it agree). A grant whose lease lapsed answers `401` like no grant,
and the next grant minted deletes it. That is how grants a stopped engine left are cleaned up: nothing revokes grants
when an engine starts, so a grant one loop mints as the engine comes up is never taken by another loop settling what
the last engine left, and two engines never take each other's. The cost is that a grant left by an engine that died
answers for up to a minute after its last renewal, until its lease runs out; only the stopped turn's MCP configuration
held it.

Logs name each call by session, server, tool, and call id, never by its arguments, which can be large.

## Turn summaries

Conversations read like a GitHub Actions log: every prompt stays visible, and the work it started collapses to one
row headed by a one or two sentence summary, which expands to the turn's full events (`api/src/sessions/summary.rs`).

- **Which turns.** Turns that complete or fail. A cancelled, interrupted, or usage-limited turn's state says what
  happened, and the conversation shows it translated.
- **Input.** While the turn runs, its follower gathers a digest from the events it already scrubbed: the prompt (its
  first 2,000 characters), the tools called in order (the first 60 by name, then a count), how the turn ended, and the
  agent's final message and error, each cut down. Nothing is read back from the database.
- **The model.** What the `turn_summary` agent resolves to for the session's project, the summaries kind's Haiku 5.5
  (`claude-haiku-5-5`) at low effort unless someone chose another (`docs/engine.md`, Models), read as each summary
  starts, through the runner like every turn: the official CLI on the same credential stack.
  The summary is a text turn (`TurnMode::Text`): no built-in tools (`--tools=`), no MCP servers (`--strict-mcp-config`),
  no user, project, or local settings (`--setting-sources=`), so the agents' configuration repository never steers it,
  its own system prompt, and no session kept (`--no-session-persistence`). It runs in a conversation of its own in
  `/workspace/.elysium/summaries`, so it never waits behind the session's next prompt, though it takes one of the eight
  turn slots, and holds an agent turn activity, for its few seconds.
- **Keeping it.** The answer is collapsed to one line of at most 400 characters, recorded in `turn_summaries`, and
  published as `session.summary`. The events route answers a session's summaries with its history.
- **On the project's timeline.** In the same transaction, a summary of a session with a project becomes a timeline
  record of that project, kind `turn_summary` (`api/src/sessions/timeline.rs`), attributed to it as certain by the
  session's agent (`session:<n>`), indexed in the knowledge base, and announced as `timelineRecord.created`. It is the
  workspace's, owned by the coding agent's machine user, since every session is listed to everyone, so every company
  member reads it on the project's Timeline tab, where it opens the session's conversation at that turn. One record per
  turn, deleted with the session; a session without a project keeps its summaries off every timeline. The summaries
  written before are put there by the knowledge base's backfill. See `docs/knowledge.md`, Turn summaries.
- **Nothing wakes on it.** A new record on a project wakes its Project Manager (`docs/engine.md`, Project Managers),
  except what Elysium's agents put there, a summary included: a wake per turn would spend a Project Manager's turn on
  every prompt anyone types into a session of the project. The work a Project Manager dispatched reports back through
  its batch's result, and it reads the rest on its timeline (`manager_timeline`) when something else wakes it. Elysia's
  intake leaves summaries out too; a session's work is followed in its own conversation.
- **Failure.** A summary that fails (no credential, an empty answer, a database error) is logged as
  `sessions.summary.failure` and left out. The row then shows the first line of the agent's final message, or the
  turn's state. It never touches the turn's own outcome.

## Media

The workspace image carries `ffmpeg`, from Ubuntu's archive. Agents use it for their own audio and video work, and
Elysium cuts meeting recordings into segment clips with it, in each meeting's scratch directory under
`.elysium/meetings/` (`docs/meetings.md`).

## Blender

Every agent can 3D model: model, render, and inspect `.blend` files through Blender's MCP tools (`mcp__blender__*`).
Blender 5.2.2, Blender Lab's MCP server, and its bridge extension are baked into the workspace image
(`workspace/blender/install.sh`, with the MCP server's hash-locked dependencies in `requirements.txt`), every download
pinned and checksum- or commit-verified.

Each turn's `blender` MCP server is `elysium-blender-mcp`, a stdio launcher Claude Code starts for the turn and stops
when it ends. It starts headless Blender with the bridge on a free loopback port, waits for it to listen, then runs the
MCP server against it, so every turn models in a Blender of its own and no two turns share a scene.

- A scene lives only as long as its turn. Work that must outlast the turn is saved to a `.blend` in the session's
  directory.
- Turns separate processes and scenes, not privileges. Every agent runs as `agent`, so one could dial another running
  turn's loopback port.
- There is no window, so the screenshot and navigation tools fail. Rendering works.
- After a Studio turn, `/opt/elysium/bin/export-glb` exports each `.blend` the turn changed to a `.glb` beside it; see
  `docs/studio.md`.

## The desktop

Every turn has a desktop of its own: a screen the agent drives with mouse, keyboard, and screenshots, for what no web
page, CLI, or API covers. Its tools are the `desktop` MCP server's (`mcp__desktop__computer`), from
[computer-use-mcp](https://github.com/domdomegg/computer-use-mcp), whose one `computer` tool mirrors Anthropic's own
computer use tool, which Claude is trained on: `key`, `type`, `mouse_move`, the clicks, `left_click_drag`, `scroll`,
`get_screenshot`, and `get_cursor_position`.

| Part              | What                                                                   |
|-------------------|------------------------------------------------------------------------|
| Display           | Xvfb at 1280 by 800, 24-bit, the size Claude's computer use is tuned for |
| Window manager    | Openbox                                                                |
| Browser           | Firefox ESR 153.4.0, from Mozilla's tarball, checksum-verified; updates, telemetry, and first-run pages are off by policy |
| Control           | computer-use-mcp 1.8.0 on Node, installed with `npm ci` from `workspace/desktop/package-lock.json`, so every transitive version is pinned; it types through `xdotool`, so text follows the keyboard layout |
| View              | x11vnc, view-only, on a Unix socket alone (no TCP port)                |

Everything is installed by `workspace/desktop/install.sh` into the workspace image. Each turn's `desktop` server is
`elysium-desktop-mcp`, a stdio launcher Claude Code starts with the turn. It starts Xvfb on a free display number, then
Openbox, x11vnc on the session's socket (`/workspace/.elysium/desktops/session-<id>.sock`, `api/src/sessions/desktop.rs`),
and Firefox on a fresh profile, then runs computer-use-mcp against the display. The runner stops the turn's process
group when the turn ends, which takes the whole desktop with it, so no two turns share a screen and nothing a turn left
open reaches the next. A session's turns run one after another, so its socket names one desktop at a time.

- A desktop lives only as long as its turn. Work that must outlast it, such as a download, is saved in the session's
  directory.
- Turns separate screens, not privileges, as with Blender: every turn runs as `agent`.
- Firefox's content sandbox cannot create user namespaces inside the workspace container and runs with the reduced
  sandbox Firefox falls back to. The workspace container is the boundary that matters.
- Each desktop costs about 300 MB while Firefox is open; eight concurrent turns are the ceiling.

### Watching it live

A conversation's Watch desktop button shows the running turn's desktop as the agent works. The view is noVNC 1.7.0 in
the web app, view-only, speaking VNC over one WebSocket, `GET /api/v1/coding-sessions/{id}/desktop`
(`api/src/routes/v1/coding_sessions/watch_desktop.rs`). The API answers it only from Elysium's own origin, checks that
the session's desktop socket is up (`404` otherwise, which is the case between turns), and relays bytes between the
WebSocket and a `docker exec` of `socat` bridged to the socket (`api/src/runner/socket.rs`,
`crate::sessions::desktop::relay`). Nothing in the workspace listens on the network, and nothing new is published.

- **View-only twice over.** x11vnc runs `-viewonly`, so nothing a viewer sends moves the agent's mouse or types into its
  screen, and the client is view-only too.
- **It ends with the turn.** When the turn's desktop stops, the socket closes and the view says so; reconnecting
  during the next turn shows that turn's desktop.
- **Who may watch.** Anyone who may see the session, as with its conversation. Password fields are masked on the
  screen as in any browser, and the vault's secrets never appear there in the clear.
- **Not Neko.** The remote browser's WebRTC streaming (`docs/browser.md`) captures the X display inside each Neko
  container. A turn's desktop has to live in the workspace, beside the agent's files and tools, and streaming it
  through Neko would mean a container per turn and a media port from the eight sign-in browsers share. VNC over the
  API's own WebSocket needs neither.

## Projects

Every Coding session belongs to one project, chosen when the session is created. Projects have a unique name and
a description, and are managed on the Projects page in the sidebar. A project cannot be deleted while any
session belongs to it: each session owns a directory and the history of its turns, which are removed deliberately by
deleting the session. A Studio item's sessions have the item's project, which is optional (see `docs/studio.md`); the Coding
area lists only sessions that belong to no Studio item.

## Sessions

Sessions are numbered 1, 2, 3, ... in the order they are created, and the number is the session's id everywhere:
routes, events, the workspace directory, and the Coding page's URLs. Creating a session reserves its number from the
table's identity sequence (`nextval`), records the row, and clones its repositories. A create that fails after the
reservation leaves a gap in the numbering; numbers are never reused.

A create may carry the first prompt, which the API queues as the session's first turn once the row is recorded and
the repositories are cloned. If a clone fails, the API removes the directory and the row instead of leaving either
behind, and answers `502` with git's message, so a create yields a usable session or nothing.

A session can be started from an action item. It then belongs to one of the item's projects, or any project when the
item has none, records the item, and requires a first prompt, which follows the item's context in the first turn. See
[Coding sessions](action-items.md#coding-sessions).

### Repositories

A session clones up to 16 repositories (https, http, ssh, or `git@` URLs), in the order given, each into the
session's `repos/` directory under a name taken from the URL's last segment without `.git`. Each may name the base
branch its work starts from; without one the remote's default branch is used.

- **Limit.** Clones run one after another before the create answers, within 30 minutes in all. Sixteen
  (`MAX_REPOSITORIES`) covers a service with its libraries and keeps that bounded.
- **Directory names.** Two repositories that would share a directory, ignoring case, answer `400` naming both URLs.
  The same repository listed twice (github.com names compared ignoring case, over HTTPS or SSH alike) is refused the
  same way and says so.
- **SSH.** Elysium holds no SSH keys: a github.com SSH remote is cloned over HTTPS with the session's GitHub token
  (see `docs/github.md`), and an SSH remote on any other host is refused.

The New session form mirrors every rule before sending (`createSessionFormSchema.ts`, `sessionRepositories.ts`).

Deleting a session cancels its turns, removes its directory, and takes its kept history with it. A Studio item's
sessions are deleted only with their item.

## History

`session_events` keeps every event a session's turns reported, as the runner's own event in JSON (`RunnerEvent`),
with the turn it belongs to. It is rendered into the view clients show (`api/src/sessions/views.rs`) when it is read
or published, so a later build renders old events with whatever it knows then.
`GET /api/v1/coding-sessions/{id}/events` reads the latest 5000, and clients merge history with live events on
`sequence`. The view kinds are `turnStarted`, `agentMessage`, `agentThinking`, `toolStarted`, `toolCompleted`,
`turnCompleted` (with the turn's state: `completed`, `failed`, `cancelled`, `interrupted`, or `usage-limited`), and
`incident` for a credential rollover. Rate limit notices and lines the runner did not understand are kept too, and
left out of the conversation.

## Frontend

`frontend/src/pages/Coding/` holds the page. It is a Dockview workspace (`dockview-react`) with two panel types:

- **Sessions** is the overview: every session, its number, project, what it is doing now, and last activity, as a
  table (uikit's `SmartTable`) or as tiles, switched from the panel's toolbar and remembered per browser. A title, or
  anywhere on a tile, opens that session's conversation. See `docs/frontend.md`.
- **Conversation** is one session, headed by its title and number ("Session 12"), and by a link to the action item
  it was started from, if any. It loads history through SWR when it opens and drops its events from
  Redux when it closes; a reopened panel shows SWR's buffered history while the fresh copy loads. Prompts and agent messages
  render as chat bubbles; tool calls, thinking, and turn results as compact lines with details folded away. Each
  turn's work sits under its prompt behind a row with its summary (see Turn summaries): open while the turn runs,
  closed once it ends, and opened again by clicking it (`TimelineEventList`, grouping in `timelineTurns.ts`). Event
  types without a renderer show their wire name. Ctrl or Cmd with Enter sends the composer's prompt; Enter adds a
  line (`docs/frontend.md`, Hotkeys).

The first conversation opens beside Sessions; later ones open as tabs in its group. Panels can be dragged, split,
and floated. The layout is saved per browser under `elysium.coding.layout.v2`; Reset layout clears it. A restored
panel whose session was deleted says so.

Panels render through portals, so they read page actions (open, create, rename, delete) from
`CodingActionsContext` rather than props. `src/theme/dockview.css` maps Dockview's variables to the Matter tokens,
so the workspace follows light and dark mode.

## Roadmap

- Rendering agent messages as Markdown.
- Answering an agent's questions and deciding proposed plans from the conversation.
- Cancelling a running turn from the conversation.
