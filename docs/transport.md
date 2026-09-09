# Transport: two contracts, one boundary each

Elysium speaks two wire contracts, and which one applies is decided by the boundary, never
by taste.

| Boundary | Contract | Owner |
|---|---|---|
| Browser to Elysium | REST at `/api/v1`, OpenAPI 3.1, a generated TypeScript client | Anubis |
| Elysium to a satellite | Protobuf over HTTP plus a WebSocket | [arsox-satellites](https://github.com/JalapenoLabs/arsox-satellites) |

## Why the browser boundary is REST

Anubis generates its OpenAPI document from the Rust handlers with utoipa, renders it as
typed ky route functions, and fails CI when the client drifts from the API. The application
merges its own models into that document, so a scaffolded endpoint arrives already typed on
both ends.

Putting protobuf on this boundary would mean giving up the generated client, the OpenAPI
document, the drift gate, and every scaffolded route module, and rebuilding all four here.
That is the divergence the Anubis-first rule exists to prevent, so the framework's contract
wins. See [anubis.md](anubis.md).

The practical cost is real and worth naming: proto3 explicit presence is expressive in a way
JSON is not, and an `Option<T>` that somebody flattens to `T` turns "not reported" into a
value. Wire types keep `snake_case` field names, because those names are the contract, and a
nullable column stays nullable all the way into the form control.

## Why the satellite boundary is protobuf

That contract is not Elysium's to choose. A satellite normalizes Claude CLI and Codex CLI
events into one shape, and the shape is defined in `arsox-satellites/proto` and consumed
through its published SDKs. Elysium is a client of that contract and never re-implements it.
See [satellites.md](satellites.md).

Two properties of it matter to Elysium's own design:

- **Every event carries a monotonic sequence and is persisted before it is sent**, so
  reconnecting with `from_sequence` costs nothing and a restarted Elysium catches up.
- **Money is never a float anywhere in the contract**, and it must not become one here. A
  fraction of a cent accumulated in a float is how a ceiling drifts away from the invoice it
  was meant to predict.

## Timestamps

Every instant is stored and transmitted in UTC and rendered in the viewer's zone at the
edge. Postgres holds `timestamptz`; the display timezone is a property of the user, not of
the row. The satellite contract carries its own `Timestamp` and the SDK converts it, so the
rule holds across both boundaries without either one having to know about the other.

## Credential transport

Distinct from either wire format, and covered in
[integrations.md](integrations.md): git over SSH and HTTPS, `docker login`, and SMTP AUTH
present a credential rather than running an OIDC dance. Elysium mints per-user SSH keys and
scoped access tokens and provisions them into the managed service, so `git push` and
`docker login` work without a hand-made personal access token. The same minting layer serves
Elysia exactly as it serves a person.
