# Elysium

One self-hosted workspace instead of six subscriptions.

Elysium is an open-source business workspace you run on your own machine: mail, chat,
storage, source control, tasking, automation, and an agentic AI, unified rather than stapled
together. The AI inside it is **Elysia**.

## Why

Google Workspace and Microsoft Teams are several products rented to you as one. The storage is
a bolt-on, the admin console is bad, every tool models a team differently, and none of it runs
on hardware you control.

- **Your machine, your keys, your network.** Expose it however you like; SSL and hosting are
  yours, everything else is handled.
- **Yours or theirs, or both at once.** Run the managed git server, or link the GitHub you
  already use, or both, aggregated into one surface. Same for mail, storage, and the registry.
- **Capacity is whatever disk you point it at.** Storage is a live gateway onto your
  providers, not a mirror with a quota.
- **We orchestrate, we do not re-implement.** Elysium provisions best-of-breed open-source
  containers (Forgejo, Postfix and Rspamd, a registry, Astraval) and owns the unification. A
  hand-rolled SMTP server is a security incident with a roadmap.
- **Things come to you.** Email and chat in one action center, items resolved rather than
  read, and `ctrl+P` to anything: a person, a message, a file, a repository.
- **AI-first, not AI-bolted-on.** The Elysia session is the primary screen, with the same
  tools and the same credentials a person has.
- **Real multi-tenancy.** Organizations, projects, teams, and roles, because a workspace is
  not a single-player tool.

## Status

Early, and honest about it. The application is stamped from the
[Anubis](https://github.com/JalapenoLabs/Anubis) framework and boots to a working
authenticated, multi-tenant app with billing, webhooks, background jobs, realtime channels,
notifications, and an audit log, all from the framework.

Elysium's own pillars are designed, not built. The domain model is blocked on a tenancy tier
being added to the framework ([Anubis #93](https://github.com/JalapenoLabs/Anubis/issues/93)),
because Elysium's resources belong to a **project** and Anubis has only organizations and
teams. See [docs/anubis.md](docs/anubis.md) for the full register of what the framework still
owes, and [docs/product.md](docs/product.md) for the build order.

## The repositories

| Repository | Role |
|---|---|
| **elysium** (this one) | The workspace and its control plane |
| [Anubis](https://github.com/JalapenoLabs/Anubis) | The Rust and React SaaS framework Elysium is built on |
| [arsox-satellites](https://github.com/JalapenoLabs/arsox-satellites) | The agent execution plane: satellite images, the protobuf contract, the SDKs |
| [uikit](https://github.com/JalapenoLabs/uikit) | Shared Jalapeno Labs React components, hooks, and utilities |

## Development

```sh
yarn install
yarn dev
```

`yarn dev` copies `.env.example` to `.env` on its first run, starts Postgres in Docker and
waits for its healthcheck, then runs the backend and the Vite dev server. The app is at
<http://localhost:5173>.

Checks:

```sh
cargo fmt --all && cargo clippy --workspace --all-targets && cargo test --workspace
yarn typecheck && yarn lint && yarn test
yarn e2e
```

## Documentation

`docs/` holds one document per decision category.

- [Architecture](docs/architecture.md) and [the product](docs/product.md)
- [Anubis](docs/anubis.md): how the framework is consumed, and the live blocked register
- [Tenancy](docs/tenancy.md): organization, project, team, individual
- [Storage](docs/storage.md) and [integrations](docs/integrations.md)
- [Mail](docs/mail.md), [OIDC provider](docs/oidc.md), [OAuth client](docs/oauth-client.md), [transport](docs/transport.md)
- [Satellites](docs/satellites.md): the agent execution plane
- [Workspace](docs/workspace.md), [navigation](docs/navigation.md), [UX](docs/ux.md), [desktop](docs/desktop.md)
- [Networking](docs/networking.md) and [CI](docs/ci.md)

## License

MIT. Elysium is a [Jalapeno Labs](https://github.com/JalapenoLabs) project.
