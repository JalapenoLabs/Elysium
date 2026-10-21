# Elysium

Rust API, Vite/React frontend, Postgres, and Redis behind a single nginx origin.

## Run

```sh
cp .env.example .env   # then fill in credentials and ELYSIUM_ENCRYPTION_KEY, see docs/secrets.md
docker compose up --wait
```

That runs the published image. Set `COMPOSE_PROFILES=auto-update` in `.env` and the deployment applies each new
release on its own, rolling back one that does not come up healthy; see `docs/infrastructure.md`.

To develop, build from this checkout and run the web app on the Vite dev server with hot reload:

```sh
docker compose -f compose.yml -f compose.dev.yml up --build --wait
```

The stack answers on `http://localhost:4000`:

| Path               | Serves                                                     |
|--------------------|------------------------------------------------------------|
| `/`                | Web app                                                    |
| `/api/ok`          | `ok`                                                       |
| `/api/ping`        | `pong`                                                     |
| `/api/version`     | Build, toolchain, and git history JSON                     |
| `/api/v1`          | The API, behind a Kratos session; see `docs/api.md`        |
| `/api/identity/`   | Ory Kratos's public API: sign-in, sign-up, and recovery    |
| `/api/mcp`         | The MCP server for Claude Code, Codex, and other clients   |

`oauth-broker/` is the separately deployable OAuth broker Gmail and Outlook connect through; see its README.

Design notes live in [`docs/`](docs/). Project rules live in [`CLAUDE.md`](CLAUDE.md).

## License

Elysium is licensed under the [Apache License 2.0](LICENSE).
