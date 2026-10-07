# Elysium

Rust API, Vite/React frontend, Postgres, and Redis behind a single nginx origin.

## Run

```sh
cp .env.example .env   # then fill in credentials and ELYSIUM_ENCRYPTION_KEY, see docs/secrets.md
docker compose up --wait
```

That runs the published image. To develop, build from this checkout and run the web app on the Vite dev server with
hot reload:

```sh
docker compose -f compose.yml -f compose.dev.yml up --build --wait
```

The stack answers on `http://localhost:4000`:

| Path           | Serves                                   |
|----------------|------------------------------------------|
| `/`            | Web app: sidebar, topbar, and pages      |
| `/settings`    | Settings directory, including LLMs       |
| `/api/ok`      | `ok`                                     |
| `/api/ping`    | `pong`                                   |
| `/api/version` | build, toolchain, and git history JSON   |
| `/api/v1/llms` | LLM credentials, tokens stored encrypted |
| `/api/v1/mail` | Mailboxes, mail server, and domains      |

`oauth-broker/` is the separately deployable OAuth broker Gmail and Outlook connect through; see its README.

Design notes live in [`docs/`](docs/). Project rules live in [`CLAUDE.md`](CLAUDE.md).

## License

Elysium is licensed under the [Apache License 2.0](LICENSE).
