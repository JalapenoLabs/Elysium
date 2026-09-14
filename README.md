# Elysium

Rust API, Vite/React frontend, Postgres, and Redis behind a single nginx origin.

## Run

```sh
cp .env.example .env   # then fill in credentials and ELYSIUM_ENCRYPTION_KEY, see docs/secrets.md
docker compose up --build --wait
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

Design notes live in [`docs/`](docs/). Project rules live in [`CLAUDE.md`](CLAUDE.md).
