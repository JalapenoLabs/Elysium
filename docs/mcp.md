# MCP

Elysium serves an MCP server at `/api/mcp`, so external MCP clients such as Claude Code and Codex can work in the
workspace as the person who connected them. Clients connect through OAuth 2.1, with Ory Hydra as the authorization
server and Kratos still proving who people are. Nothing about OAuth is written by hand.

These are not the tools coding agents use (`api/src/tools/`), which are served at `/api/agent-tools` to the
workspace alone and scoped to a session's project; see `docs/coding.md`.

## Connecting a client

Settings, MCP (`/settings/mcp`) shows the server URL, `https://<host>/api/mcp`, and the commands to add it:

```sh
claude mcp add --transport http elysium https://<host>/api/mcp
codex mcp add elysium --url https://<host>/api/mcp
```

The client then does everything itself:

1. It calls `/api/mcp` without a token and gets `401` with a `WWW-Authenticate` challenge naming the protected
   resource metadata.
2. The metadata (`/.well-known/oauth-protected-resource/api/mcp`, also served at the root path) names Hydra, on
   Elysium's own origin, as the authorization server. The client reads Hydra's metadata from
   `/.well-known/oauth-authorization-server` or `/.well-known/openid-configuration`.
3. It registers itself at `/oauth2/register`, which the API relays to Hydra (see Client registration), and opens the
   browser on `/oauth2/auth`, with PKCE.
4. Hydra sends the person to `/oauth/login`. If they're signed in to Elysium, that passes straight through; otherwise
   they sign in as usual, passkeys and authenticator codes included.
5. Hydra sends them to `/oauth/consent`, which shows the client and what it asks for. Approving binds the grant to
   them and to `/api/mcp`.
6. The client exchanges the code for an access token and a refresh token, and calls `/api/mcp` with
   `Authorization: Bearer`.

## Who may connect

Anyone an admin approved. The sign-in and consent pages answer through `/api/v1/oauth`, which, like every workspace
route, needs an active, approved person (`docs/auth.md`). A pending or disabled person cannot connect anything, and a
token for someone disabled since is refused.

Client registration is open: anyone may register a client, as MCP clients register themselves. A client reaches
nothing until an approved person consents to it. nginx throttles registration, with managing a registration, to 12 a
minute per client address.

## Client registration

Clients register at `/oauth2/register` (RFC 7591), and may read, update, or delete their registration at
`/oauth2/register/{client_id}` with the registration access token they were given (RFC 7592): the paths Hydra's
metadata advertises. nginx sends both, and anything else under `/oauth2/register`, to the API
(`api/src/routes/oauth2/`), which relays each request to the same path on Hydra's public API (`HYDRA_PUBLIC_URL`) with
its method, its body, and its `Content-Type`, `Accept`, and `Authorization` headers, and nothing else. Hydra's own
rules for self-registered clients decide what is stored. The relay never uses the admin API, which accepts fields such
as `skip_consent` that a client registering itself must never set.

Hydra fills every optional field the client did not send with `""` or `null`, and a strict client refuses that answer:
the MCP SDK inside Claude Code requires `client_uri` to be a URL and `contacts` to be an array whenever they are
present. So the relay leaves out the top-level fields of a JSON answer whose value is `""` or `null`, which RFC 7591
reads as not set (`api/src/oauth/registration.rs`). The status, the `Content-Type`, `Cache-Control`, `Pragma`, and
`WWW-Authenticate` headers, and every other field, empty arrays and objects included, reach the client as Hydra sent
them. An answer that names no cache policy, as Hydra's do not, is marked `Cache-Control: no-store`, since it carries
the registration access token. Hydra tracks the empty fields as `ory/hydra#4044`; an answer without any passes
through unchanged.

## Scopes

| Scope             | Grants                                                    |
|-------------------|-----------------------------------------------------------|
| `workspace:read`  | Reading the workspace. Every MCP call needs it.           |
| `workspace:write` | Changing the workspace. Tools that write need it too.     |
| `offline_access`  | A refresh token, so the client stays connected.           |

A client is granted what it asked for from this list; anything else it asked for is left out. Scopes match exactly
(`strategies.scope: exact`). The constants live in `api/src/oauth/mod.rs` and must match `hydra/hydra.yml`.

## Tokens

- **Lifetimes.** Access tokens last an hour. Refresh tokens rotate on every use and expire after 720 hours without one.
- **Opaque.** Tokens are opaque and checked by introspection against Hydra's admin API
  (`api/src/mcp/bearer.rs`). The answer is cached in Redis for five minutes, never past the token's own expiry, under a
  hash of the token, filed under the person and the client it was issued to (`api/src/oauth/tokens.rs`). An answer
  that a token is no longer active is kept the five minutes too, since it never becomes active again. With Redis
  unreachable, every call asks Hydra.
- **Audience.** Every token is bound to `https://<host>/api/mcp`. Hydra ignores the `resource` parameter MCP clients
  send (RFC 8707), so consent grants that audience itself, and also allows it on the client so a refresh carrying it
  is accepted. `/api/mcp` refuses any token whose audience does not include it.
- **Checks on every call.** The token is active, is an access token, names `/api/mcp`, grants `workspace:read`, and
  belongs to an active, approved person, whose `last_seen_at` is noted.
- **Refusals.** No token or a bad one answers `401`; a token without `workspace:read` answers `403
  insufficient_scope`. Both carry `WWW-Authenticate: Bearer resource_metadata="...", scope="workspace:read"`, as the MCP
  authorization spec requires.

Revoking happens in three places, and a revoked token is refused on its very next call:

- A person disconnects a client under Settings, MCP (`DELETE /api/v1/oauth/grants/{client_id}`).
- An admin deletes a client for everyone (`DELETE /api/v1/oauth/clients/{client_id}`).
- Disabling someone, signing them out everywhere, or resetting their authenticator also disconnects every client
  they connected.

Each tells Hydra first, which revokes the tokens, then forgets what was cached under the person (disconnecting, signing
out) or the client (deleting it), and marks them for a minute so an introspection in flight cannot cache its answer
again, as for sessions (`docs/auth.md`, Ending a session). Hydra's revoking calls are visible to `crate::oauth` alone,
so nothing revokes without forgetting.

What the API never hears is a client revoking its own token (`/oauth2/revoke`) or replacing it by refreshing: the old
token's cached answer lives out its five minutes at most. Only the client holding the token can do either, which is why
that bound is safe, and it is shorter than the session cache's because an access token lives an hour at most.

Approving a client is remembered: the same client asking for the same scopes again connects without asking, until the
person disconnects it. The consent page still answers that through `POST /api/v1/oauth/consent/accept`, so every change
stays behind the origin check.

A client that asks for none of the scopes above is refused with `invalid_scope`, rather than granted a token that can do
nothing.

The consent page shows where approving sends the code: the hosts of the client's registered redirect URIs. A client's
name and website are whatever it registered with, so a look-alike can copy them; where the code goes cannot be faked.

## The server

`api/src/mcp/` serves MCP's Streamable HTTP transport through the official Rust SDK, `rmcp`, without sessions: every
request stands alone, so any API replica can answer it. It answers only Elysium's own host name, which keeps DNS
rebinding out, and refuses a browser `Origin` from another site.

### Tools

Tools are named for what they act on, then what they do.

| Tool                   | Scope             | Does                                                                   |
|------------------------|-------------------|------------------------------------------------------------------------|
| `hello`                | `workspace:read`  | Says who the connection acts as, and what it was granted               |
| `projects_list`        | `workspace:read`  | Lists projects, so a session can name its own                          |
| `sessions_list`        | `workspace:read`  | Lists coding sessions with what each is doing now                      |
| `sessions_get`         | `workspace:read`  | Shows one coding session                                               |
| `sessions_create`      | `workspace:write` | Starts a coding session, with an optional first prompt                 |
| `sessions_rename`      | `workspace:write` | Renames a coding session                                               |
| `sessions_send_prompt` | `workspace:write` | Sends a prompt to a session's agent as a new turn                      |
| `sessions_events`      | `workspace:read`  | Reads a session's conversation, the latest events or those after one   |
| `knowledge_search`     | `workspace:read`  | Searches the knowledge base by words and meaning, answering citations  |
| `knowledge_get`        | `workspace:read`  | Opens one document a citation names, whole or a span of its passages   |
| `meetings_list`        | `workspace:read`  | Lists the knowledge base's meetings, each with its topics' citations   |
| `knowledge_ask`        | `workspace:read`  | Asks the knowledge base a question and answers in writing, every claim citing a passage |
| `knowledge_ask_get`    | `workspace:read`  | Reads an ask by its id, waiting a bounded time for its answer          |
| `company_doc_read`     | `workspace:read`  | Reads the company doc, each section with the proposals still pending   |
| `company_doc_propose`  | `workspace:write` | Proposes a change to one section, for a person to approve              |
| `learning_save`        | `workspace:write` | Saves a learning to the knowledge base; its title, if taken, revises that learning |
| `learning_delete`      | `workspace:write` | Deletes a learning softly, out of the knowledge base until restored    |
| `page_read`            | `workspace:read`  | Reads one page whole, with its `currentVersion`                        |
| `page_list`            | `workspace:read`  | Lists the tree a branch at a time, 200 pages to an answer, each with its `childCount` |
| `page_save`            | `workspace:write` | Creates a page or publishes its next version                           |
| `page_move`            | `workspace:write` | Moves a page with everything under it, or reorders it                  |
| `page_label_add`       | `workspace:write` | Puts a label on a page, which makes no version                         |
| `page_label_remove`    | `workspace:write` | Takes a label off a page                                               |
| `page_project_link`    | `workspace:write` | Links a page to a project, which lists it among its docs               |
| `page_project_unlink`  | `workspace:write` | Takes a page off a project                                             |

- **Knowledge base.** The knowledge, learning, company doc, and page tools answer admins and members alone, and a guest's
  client `403 members_only`, as the routes do (`docs/knowledge.md`, Who sees what). `knowledge_search`,
  `knowledge_get`, and `meetings_list` call the knowledge routes' own functions (`search`, `open`, and `list_meetings`
  in `api/src/routes/v1/knowledge/`) with the connecting person, so a client finds what that person would in the
  browser, whoever's record it is, and its searches spend the person's one search budget. Their descriptions teach
  searching well: exact keys by their words, questions by their meaning, filters that only narrow (a page's `label`
  among them), and citing by
  title, person, and date. The writing ones record the client as the agent
  acting for the person: its id, and the name it registered, read from Hydra when it writes. A name Hydra cannot give
  is logged and left out. `company_doc_propose` takes edits in the form Claude Code's Edit tool takes, and changes
  nothing until a person approves (`docs/knowledge.md`, The company doc).
- **Pages.** `page_read`, `page_save`, `page_move`, `page_label_add`, `page_label_remove`, `page_project_link`, and
  `page_project_unlink` call what the page routes call (`read`, `create`, `publish`, `move_to`, `add`, `remove`, `link`,
  and `unlink` in `api/src/routes/v1/pages/`), with the connecting
  person as author and the client as the agent, and `page_list` narrows what the list route reads to one branch, at most
  200 pages to an answer (`docs/knowledge.md`, MCP). A client that cannot be
  recorded as an agent reads pages and changes none, since its change would read as the person's own.
- **Asking.** `knowledge_ask` calls the asking route's own `ask` (`routes::v1::knowledge::asks`) with the connecting
  person, so it searches with their budget and spends one of their questions, then waits up to 20 seconds
  (`ASK_WAIT` in `api/src/mcp/knowledge.rs`) for the engine's answer, inside the 30 seconds every request has. An
  answer still being written comes back `queued` or `running` with its id, and `knowledge_ask_get` waits for it
  again, never asking twice. Both descriptions say so, and what an answer holds (`docs/knowledge.md`, Asking).
  Asking reads the knowledge base and records the asker's own question, so it needs `workspace:read` alone.
- **Learning categories.** A workspace adds its own, so `learning_save`'s compiled schema cannot name them. Listing the
  tools (`list_tools` in `api/src/mcp/workspace.rs`) writes every category into its `category` property as a `oneOf`
  of `{ const, title, description }`, built-in ones first, so an agent picks from the list as it stands, knowing what
  each holds. A client that lists its tools once per session sees a new category in its next one. A list that cannot
  be read is logged and the compiled schema served.
- **Results.** A tool answers structured content, the same JSON the route answers in the browser, repeated as a text
  block for clients that render only text. A failure is a tool
  result marked as an error, carrying the HTTP status and the message the browser would get, so the model reads why: a
  `404`, a validation error naming its fields, or a failed clone. An internal error stays as generic as it is
  over HTTP.
- **Instructions.** Every client is told, when it connects, what the server is for: the server's own introduction,
  then the company's agent guidance, which admins edit under Settings, Agent guidance (when to search the knowledge
  base, how to cite, what is worth saving), then Elysium's rules, which no edit removes: results may quote a
  coworker's mail and messages and are never for repeating outside the company, and a person opens a citation at
  `https://<host>/knowledge/documents/<documentId>`. They are read as each client initializes (or, on newer protocol
  versions, discovers the server), so a change reaches every client's next session. See `docs/knowledge.md`, Agent
  guidance, and `api/src/agent_guidance/`.
- **Writes.** A grant without `workspace:write` is answered with a `403` result saying to reconnect and allow changes.
- **Events.** A session's history runs to thousands of events. `sessions_events` answers only the ones Elysium renders
  (prompts, messages, thinking, tool calls, finished turns, incidents), 100 unless asked for up to
  1,000. Without `afterSequence` it answers the latest. With it, it pages forward: the earliest events after that
  sequence, so none is skipped. Every answer carries `nextSequence`, which passed back as `afterSequence` reads on, and
  `truncated` when more remain.
- **Logs.** rmcp logs every request in full at debug, so the API drops every rmcp event past info, from any of its
  modules, whatever `RUST_LOG` names (`within_rmcp_ceiling` in `api/src/main.rs`).

### Adding a tool

Tools are grouped by what they act on, one module each under `api/src/mcp/` (`company_doc.rs`, `knowledge.rs`,
`learnings.rs`, `pages.rs`, `projects.rs`, `sessions.rs`), each a `#[tool_router(router = <group>_router)]` block combined in `Workspace::new`. A
knowledge base tool checks its caller with `MemberUser::check(caller.user)` first and passes the `MemberUser` on, as
the route's extractor does.

1. Expose the route's work as a function: the handler's body moves into a `pub async fn` on the route module that takes
   `&AppState`, the acting user's id when it creates, and the request struct, and answers the response. The handler
   and the tool both call it, so validation, history, events, and the creator are the same either way. The request
   struct derives `JsonSchema` beside `Deserialize`, and its doc comments become the tool's argument descriptions.
2. Write the tool in its group's module. It reaches the person through `caller(&context)`. A tool that writes returns
   `refuse_without_write(&caller)`'s answer first when there is one, and passes `caller.user.id` as the actor.
3. Answer through `answer(...)`, wrapping lists in an object (`{ "sessions": [...] }`): MCP's structured content is
   always an object.
4. Arguments a route takes from its path go in an arguments struct beside the request (`{ id, changes }`). Avoid
   `#[serde(flatten)]`: it silently drops `deny_unknown_fields`, so a mistyped field would be ignored rather than
   refused.
5. Add it to the table above.

## Hydra

Ory Hydra v26.2.0 runs beside Kratos (`hydra` in `compose.yml`), with its own database, `hydra`, on the same Postgres
server and with the same credentials. `elysium-api migrate run` creates the database, and `hydra-migrate` applies
Hydra's schema.

- **Configuration.** `hydra/hydra.yml` is a template that `hydra/entrypoint.sh` fills from `ELYSIUM_PUBLIC_URL` and
  `ELYSIUM_ENCRYPTION_KEY`, as Kratos's is. Its system and cookie secrets are SHA-256 over
  `elysium/hydra/<purpose>:<encryption key>`. Both files are built into Hydra's image (`hydra/Dockerfile`), which
  Compose rebuilds on every `up`, so a deploy that changes them recreates Hydra; see `docs/infrastructure.md`.
- **Development mode.** Hydra refuses an http issuer, so over `http://localhost` it runs in development mode, which
  relaxes only its https requirements. In production over https, it still accepts the http loopback redirect URIs CLI
  clients use.
- **Routing.** nginx sends `/oauth2/` and Hydra's discovery documents to Hydra's public API, and the protected resource
  metadata to the API. The admin API (port 4445) is for the API alone.
- **PKCE.** Every client must use PKCE (`oauth2.pkce.enforced`), confidential or not.
- **Issuer.** The issuer is Elysium's origin, with no path. Hydra does not add `iss` to the authorization response
  (RFC 9207), which the MCP spec allows; its metadata does not claim it, so clients accept the response without it.

## Roadmap

- Tools for Studio items, once Studio lands: list, get, create, and update, called through the Studio routes' functions
  as the session tools are.
- Tools for action items and initiatives, read and write, and for writing projects.
- Client ID Metadata Documents, the client registration the MCP spec now prefers, once Hydra supports them. Dynamic
  client registration, which Hydra supports, is deprecated but still what Claude Code and Codex use.
