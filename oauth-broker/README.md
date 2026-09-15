# Elysium OAuth broker

A small, stateless service that lets self-hosted Elysium instances connect Gmail and Outlook accounts without
each operator registering their own Google and Microsoft OAuth apps. One deployment holds the apps; every instance
that trusts it can connect mailboxes through it.

It stores nothing. State that must survive a browser round trip is sealed with XChaCha20-Poly1305 under
`BROKER_SEALING_KEY`, so any number of replicas sharing that key can serve the same flows.

Licensed under the [Apache License 2.0](LICENSE). The protocol and its security properties are described in
Elysium's [`docs/mail.md`](../docs/mail.md).

## Routes

| Route                         | Caller           | Purpose                                                          |
|-------------------------------|------------------|------------------------------------------------------------------|
| `GET /healthz`                | orchestrator     | Liveness                                                         |
| `GET /v1/providers`           | Elysium backend  | `{ providers: ["google", "microsoft"] }`, whichever are set up   |
| `GET /v1/authorize`           | browser          | Consent interstitial naming the instance, then the provider      |
| `GET /v1/callback/{provider}` | browser          | Code exchange, then a redirect back with a sealed handoff code   |
| `POST /v1/redeem`             | Elysium backend  | `{ handoff, codeVerifier }` for `{ provider, address, refreshToken }` |
| `POST /v1/refresh`            | Elysium backend  | `{ provider, refreshToken }` for `{ accessToken, expiresIn, refreshToken? }` |

JSON errors are `{ error, message }`. `invalid_grant` means the handoff code or refresh token no longer works and
the account must be connected again.

## Run

```sh
cp .env.example .env    # fill in BROKER_SEALING_KEY and at least one provider
docker compose up --build --wait
```

The broker answers on `http://localhost:4100`. Put TLS termination in front of it before exposing it: providers
redirect only to HTTPS for anything but localhost, and the redirect URIs you register must match
`BROKER_PUBLIC_URL` exactly.

Point an Elysium instance at it by setting the API's `OAUTH_BROKER_URL` in Elysium's `compose.yml`. When the API
reaches the broker at a different address than browsers do, also set `OAUTH_BROKER_INTERNAL_URL`.

| Variable                                   | Required        | Purpose                                              |
|--------------------------------------------|-----------------|------------------------------------------------------|
| `BROKER_PUBLIC_URL`                        | yes             | Where browsers reach the broker                      |
| `BROKER_SEALING_KEY`                       | yes             | Base64 32-byte key; `openssl rand -base64 32`        |
| `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET` | one provider    | Google OAuth app                                     |
| `MICROSOFT_CLIENT_ID`, `MICROSOFT_CLIENT_SECRET` | one provider | Microsoft Entra app                                 |
| `PORT`                                     | no, 8080        | Listen port inside the container                     |
| `RUST_LOG`, `LOG_FORMAT`                   | no              | tracing filter; `json` for one object per line       |

## Registering the apps

### Google

1. In Google Cloud Console, create a project and configure the OAuth consent screen as **External**.
2. Add the scopes `openid`, `email`, and `https://mail.google.com/`.
3. Create an OAuth client of type **Web application** with the authorized redirect URI
   `<BROKER_PUBLIC_URL>/v1/callback/google`.
4. Put the client id and secret in `GOOGLE_CLIENT_ID` and `GOOGLE_CLIENT_SECRET`.

`https://mail.google.com/` is the only scope Gmail's IMAP and SMTP accept, and Google classes it as restricted.
Until the app passes Google's verification and third-party security assessment, the consent screen warns that the
app is unverified and at most 100 users can grant it. While the app is in testing, only listed test users can.

### Microsoft

1. In Microsoft Entra admin center, register an application with supported account types **Accounts in any
   organizational directory and personal Microsoft accounts**. Without personal accounts, Outlook.com users
   cannot connect.
2. Add a **Web** platform redirect URI `<BROKER_PUBLIC_URL>/v1/callback/microsoft`.
3. Under API permissions add delegated `openid`, `email`, `offline_access`, and from Office 365 Exchange Online
   `IMAP.AccessAsUser.All` and `SMTP.Send`.
4. Create a client secret and put it and the application (client) id in `MICROSOFT_CLIENT_ID` and
   `MICROSOFT_CLIENT_SECRET`. Client secrets expire; rotate before the date Entra shows.

Until publisher verification, Microsoft's consent screen marks the app unverified. Work and school tenants may also
need an administrator to allow SMTP AUTH for the mailbox.

## Develop

```sh
cargo test
cargo clippy --all-targets --locked
```

## Roadmap

- Per-IP rate limiting on the public routes.
