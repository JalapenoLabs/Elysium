# MCP

Elysium serves an MCP server at `/api/mcp`, so external MCP clients such as Claude Code and Codex can work in the
workspace as the person who connected them. Clients connect through OAuth 2.1, with Ory Hydra as the authorization
server and Kratos still proving who people are. Nothing about OAuth is written by hand.

These are not the tools coding agents use on satellites (`api/src/tools/`), which are relayed through the satellite and
scoped to a session's project.

## Connecting a client

Settings, Connected apps shows the server URL, `https://<host>/api/mcp`, and the commands to add it:

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
3. It registers itself at `/oauth2/register` and opens the browser on `/oauth2/auth`, with PKCE.
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
nothing until an approved person consents to it. nginx throttles registration to 12 a minute per client address.

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
  (`api/src/mcp/bearer.rs`). The answer is cached in Redis for 20 seconds under a hash of the token, so a revoked grant
  stops working within that time.
- **Audience.** Every token is bound to `https://<host>/api/mcp`. Hydra ignores the `resource` parameter MCP clients
  send (RFC 8707), so consent grants that audience itself, and also allows it on the client so a refresh carrying it
  is accepted. `/api/mcp` refuses any token whose audience does not include it.
- **Checks on every call.** The token is active, is an access token, names `/api/mcp`, grants `workspace:read`, and
  belongs to an active, approved person, whose `last_seen_at` is noted.
- **Refusals.** No token or a bad one answers `401`; a token without `workspace:read` answers `403
  insufficient_scope`. Both carry `WWW-Authenticate: Bearer resource_metadata="...", scope="workspace:read"`, as the MCP
  authorization spec requires.

Revoking happens in three places:

- A person disconnects a client under Settings, Connected apps (`DELETE /api/v1/oauth/grants/{client_id}`).
- An admin deletes a client for everyone (`DELETE /api/v1/oauth/clients/{client_id}`).
- Disabling someone, signing them out everywhere, or resetting their authenticator also disconnects every client
  they connected.

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
| `satellites_list`      | `workspace:read`  | Lists satellites with their latest status                              |
| `satellites_get`       | `workspace:read`  | Shows one satellite                                                    |
| `satellites_create`    | `workspace:write` | Registers a satellite by URL and bearer secret, and starts watching it |
| `satellites_update`    | `workspace:write` | Changes a satellite's name, description, URL, secret, or active state  |
| `sessions_list`        | `workspace:read`  | Lists coding sessions with their thread's latest state                 |
| `sessions_get`         | `workspace:read`  | Shows one coding session                                               |
| `sessions_create`      | `workspace:write` | Starts a coding session, with an optional first prompt                 |
| `sessions_rename`      | `workspace:write` | Renames a coding session                                               |
| `sessions_send_prompt` | `workspace:write` | Sends a prompt to a session's agent as a new turn                      |
| `sessions_events`      | `workspace:read`  | Reads a session's conversation, the latest events or those after one   |

- **Results.** A tool answers structured content, the same JSON the route answers in the browser. A failure is a tool
  result marked as an error, carrying the HTTP status and the message the browser would get, so the model reads why: a
  `404`, a validation error naming its fields, or a satellite's refusal. An internal error stays as generic as it is
  over HTTP.
- **Writes.** A grant without `workspace:write` is answered with a `403` result saying to reconnect and allow changes.
- **Events.** A thread's history runs to thousands of events. `sessions_events` answers only the ones Elysium renders
  (prompts, messages, thinking, tool calls, plans, questions, finished turns, incidents), the latest 100 unless asked
  for up to 1,000, with the thread's `latestSequence`. Passing that back as `afterSequence` reads what happened since.
- **Secrets.** A satellite's bearer secret is accepted by `satellites_create` and `satellites_update` and never
  answered. rmcp logs every request in full at debug, so the API holds its `rmcp` target at info after reading
  `RUST_LOG`: `RUST_LOG=debug`, even `rmcp=debug`, never logs one. Only naming one of rmcp's modules outright still
  does.

### Adding a tool

Tools are grouped by what they act on, one module each under `api/src/mcp/` (`projects.rs`, `satellites.rs`,
`sessions.rs`), each a `#[tool_router(router = <group>_router)]` block combined in `Workspace::new`.

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
  `elysium/hydra/<purpose>:<encryption key>`.
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
