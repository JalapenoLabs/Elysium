# Arsox

Run coding agents on hardware you control, from a browser.

Arsox is a web interface for Claude Code and Codex, in the spirit of Claude Code on the web, with
the constraints removed. You register **satellites**, which are docker containers running on your own
machines or self-hosted runners, and Arsox drives them: it queues jobs, streams the agents' work
back in realtime, answers their questions, approves their plans, and reports what everything cost.

## What makes it different

- **Your compute, your keys, your network.** A satellite runs where you put it, reaches what you let
  it reach, and bills against subscriptions and API keys you already hold.
- **One event stream, either harness.** Claude CLI and Codex CLI emit different events with
  different shapes. Satellites normalize both into a single protobuf contract, so switching harness,
  model, or provider does not rewrite the interface.
- **Teams of agents.** Opt into team mode and a commander spawns peer agents, each with its own
  context window and its own git worktree, integrating through a serialized merge queue.
- **Guardrails that are not prompts.** Exec allowlists, an egress proxy, branch protection, secret
  redaction, and token and cost ceilings are enforced by infrastructure the agent cannot reach. An
  instruction in `AGENTS.md` is advice; these are not.
- **Nothing fails silently.** Every failure at every severity is recorded as an incident with a
  stable code and a disposition, streamed live and queryable afterward, including the failures that
  recovered on their own.
- **Real multi-tenancy.** Organizations, teams, roles, and invitations, because your fleet is not a
  single-player tool.

## Status

Early. The application is stamped from the [Anubis](https://github.com/JalapenoLabs/Anubis)
framework and boots to a working authenticated, multi-tenant app; the satellite integration is not
built yet, because the satellite SDK it depends on does not exist yet. See
[docs/satellites.md](docs/satellites.md) for exactly what is missing and in what order it is needed,
and [docs/anubis.md](docs/anubis.md) for what the framework still owes its first consumer.

## Development

```sh
yarn install
yarn dev
```

`yarn dev` writes a `.env` from `.env.example` if you do not have one, starts Postgres in docker and
waits for it to pass its health check, runs the backend with that environment, and starts the vite
dev server. The backend applies pending migrations at boot. The app is at http://localhost:5173.

Email (verification, password reset, invitations, sign-in codes) goes to the backend log in
development. The action links and codes are in the log lines. There is no working transport backend
yet, in Arsox or in Anubis.

You will also want the framework CLI, which is not published to crates.io yet:

```sh
cargo install --git https://github.com/JalapenoLabs/Anubis anubis
```

### Everyday commands

```sh
yarn typecheck && yarn lint && yarn test && yarn build   # the frontend gates
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace                                   # needs DATABASE_URL

anubis doctor          # verify toolchain, database, and config health
anubis routes          # print the framework route table
anubis roles check     # validate config/roles.yml
```

## Layout

| Path | What lives there |
|---|---|
| `backend/` | The Rust application server: Axum on tokio, composed from the anubis crate |
| `backend/src/scaffolding/` | Anubis living templates. Generator input, not Arsox domain. Do not delete |
| `frontend/` | The React SPA: Vite, HeroUI, Tailwind, on `@jalapenolabs/anubis` |
| `config/roles.yml` | Roles and permissions, compiled into backend authorization and frontend affordances |
| `docs/` | One document per decision category |
| `compose.yaml` | The development Postgres |

## The three repositories

| Repository | Role |
|---|---|
| **arsox** | The web application: the control plane |
| [arsox-satellites](https://github.com/JalapenoLabs/arsox-satellites) | Satellite images, the protobuf contract, and the SDKs: the execution plane |
| [Anubis](https://github.com/JalapenoLabs/Anubis) | The Rust-first SaaS framework Arsox is built on |

Arsox is built as a full consumer of Anubis, and contributes back everything the framework turns out
to be missing rather than working around it. That rule, and how to apply it, is the first thing in
[CLAUDE.md](CLAUDE.md).

## Contributing

`main` is production (stable). `develop` is the working branch that pull requests target.
Stakeholders may push straight to `develop`; everyone else opens a pull request. Stakeholders are
responsible for promoting `develop` to `main`.

## License

MIT. Arsox is a Jalapeno Labs project.
