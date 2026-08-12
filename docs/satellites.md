# Satellites

A satellite is the thing that actually runs the agent. Arsox is its control plane and nothing more:
Arsox decides what should happen and to whom it is visible, and a satellite decides how it happens
and does it.

The contract is defined in [arsox-satellites](https://github.com/JalapenoLabs/arsox-satellites) and
that repository's README is the authority. This document covers what Arsox needs from it, how Arsox
will model it, and what is missing today.

## Current state: the contract exists, the client does not

The `arsox-satellites` repository today carries `proto/`, the committed `gen/` output for
TypeScript, Rust, and Python, and its documentation. It does not yet carry the satellite image, the
Rust SDK, the Node SDK, or the Python SDK, and there is no published crate to depend on.

**So Arsox has no satellite client to call, and the satellite integration is blocked at the first
step.** This is not a gap Arsox routes around by hand-rolling a protobuf HTTP client: the SDK is the
satellite project's deliverable, and a second implementation of it living here is exactly the
divergence the single-contract design exists to prevent.

What Arsox needs from that repository, in the order it is needed:

1. **A published Rust SDK**, or a `gen/rust` crate Arsox can depend on from git, so the backend can
   speak the contract at all.
2. **A satellite image** that can be run locally, so any of this is testable.
3. **Thread and turn endpoints**: create a thread, submit a turn, cancel a turn, read status.
4. **The thread event socket** with `from_sequence` resumption, which is what the live view is.
5. **Interaction endpoints**: answer a question set, decide a plan.
6. **Incident and usage queries**, for the reporting surfaces.

## What Arsox will model

Proposed, not built. The domain model is the highest-leverage decision in the application and the
Anubis workflow is explicit that it gets reviewed with people before it is scaffolded, so these are
written down to be argued with rather than run.

| Model | Owner | Holds |
|---|---|---|
| `Satellite` | `Team` | Name, base URL, sealed bearer secret, last-seen health, reported version and proto major, reported harness capabilities |
| `Thread` | `Satellite` | The satellite's thread id, title, state, idle TTL, budgets as created, the creating membership, and the highest mirrored sequence |
| `Turn` | `Thread` | The satellite's turn id, prompt, status, the submitting membership, and token and cost totals |
| `ThreadEvent` | `Thread` | Sequence, type, and payload, as mirrored from the socket |
| `Incident` | `Thread` | Code, disposition, retryable, message, details, occurred-at |

Ownership chains to a `Team` in every case, which is what makes the Anubis guards do the tenancy
work. Nothing is owned by a `User`.

Two things deliberately do **not** get their own model:

- **Artifacts** are listed and downloaded from the satellite on demand. Mirroring a 100 MiB artifact
  into Postgres to avoid one proxied request is the wrong trade.
- **Workspace files** are the satellite's, always. Arsox lists them through the satellite and never
  caches a tree that changes under it.

### Why the secret is a column and not a settings blob

A satellite's bearer secret is the one credential that grants total authority over that satellite,
so it is sealed with `anubis::auth::secret_box` and stored in its own column, never inside a JSON
settings document. A typed column is greppable and a reviewer can see every place it is read; a key
inside a blob is not, and the fourteenth credential added to that blob is the one that leaks. The
satellite contract makes the same call with `common.v1.Secret`, for the same reason.

The secret is never returned to a client. Responses carry a redacted display string.

## Registration, and the reachability question

Arsox is the client and the satellite is the server: Arsox dials out to a satellite's base URL and
presents `ARSOX_SECRET` as a bearer token. Registering a satellite therefore means telling Arsox its
address and its secret, and Arsox confirming it by calling `GET /v1/version` and `GET /v1/harness`
before saving the record.

That works when the satellite is reachable from wherever Arsox runs: a self-hosted runner with a
routable address, a machine on the same private network, or a home machine behind a tunnel such as
Tailscale or Cloudflare Tunnel.

**It does not work for a home machine behind NAT with no tunnel, and that is the common case.** The
alternative is inverting the connection, with the satellite dialing out to Arsox and Arsox
multiplexing commands back down that connection. That is a satellite-side protocol change, not an
Arsox feature, and it needs to be designed in `arsox-satellites` rather than assumed here. Until
then, "bring your own tunnel" is the documented answer and it should be documented honestly in the
registration UI rather than discovered by a user whose satellite never connects.

## Mirroring

For each satellite a team owns, Arsox holds one WebSocket per active thread and records the highest
sequence it has stored. Reconnection replays from `from_sequence + 1`, so a network blip costs
nothing and a restarted Arsox catches up rather than losing history.

Two failure modes come straight from the contract and both need handling on day one:

- **`STREAM_CONSUMER_LAGGED`**: Arsox fell behind and was closed. Reconnect from the last stored
  sequence. Degrading loudly is the contract working as intended.
- **`STREAM_SEQUENCE_EXPIRED`**: the requested sequence is older than the satellite's retained
  history. The mirror has a hole in it, and the honest response is to mark the gap in the mirror,
  not to renumber around it and present a stream that looks complete.

Statistics events are not emitted over the socket by default because they change rapidly. Arsox
fetches usage on demand rather than opting in.

## Budgets, permissions, and who may raise them

Budgets are required at thread creation, with an explicit unlimited value if that is genuinely what
is meant. Arsox surfaces this as a required field and never defaults it silently, because the whole
point of the contract's `oneof` between a value and an `Unlimited` message is that an unbounded
spend is a decision somebody made.

Which member may create a satellite, start a turn, answer a question, approve a plan, or set an
unlimited budget is expressed in `config/roles.yml` and nowhere else.

Permissions passed to a satellite (egress allowlist, exec allowlist, push and branch rules,
redaction override) are settings Arsox stores and forwards. Arsox does not enforce them and must
never present itself as if it does: enforcement is the satellite's, by infrastructure the agent
cannot reach. An Arsox UI that implies otherwise is lying about the security model.

## The one rule about incidents

The satellite records every failure at every severity as an incident, including the ones that
recovered on their own, and the contract states plainly that a code path which swallows a failure is
a bug. Arsox mirrors that rule: an incident that arrives on the stream is stored and shown. Arsox
does not filter incidents on the way in, because a control plane that hides failures is worse than
one that has none to show.

`recovered` incidents in particular are the ones worth surfacing rather than hiding. A failover that
works looks exactly like success, and it is being paid for on every call.
