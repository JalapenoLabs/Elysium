# Mail

Elysium reads and sends mail through mailboxes connected on the Email settings page (`/settings/email`). This doc
covers the backend: account kinds, the one transport they share, the OAuth broker Gmail and Outlook go through, and
the bundled Stalwart server. There is no mail-reading UI; mail will surface through a new kind of UI later.

## Account kinds

| Kind          | Servers                                                          | Credential                   |
|---------------|------------------------------------------------------------------|------------------------------|
| `gmail`       | `imap.gmail.com:993` TLS, `smtp.gmail.com:465` TLS                | OAuth refresh token          |
| `outlook`     | `outlook.office365.com:993` TLS, `smtp.office365.com:587` STARTTLS | OAuth refresh token          |
| `self_hosted` | the bundled Stalwart, `stalwart:993` and `stalwart:465`, TLS       | Generated mailbox password   |

Servers are fixed per kind in `api/src/mail/mod.rs`, never taken from a request, so no request can point the API at
an arbitrary host. The credential is sealed in `mail_accounts.credential_encrypted` (see `docs/secrets.md`) and
never leaves the API.

Addresses are stored lowercase and unique. Connecting an address that is already connected as the same kind
replaces its credential; connecting it as another kind is refused.

## Transport

Every kind goes through `api/src/mail/transport.rs`: `async-imap` for IMAP and `lettre` for SMTP, both on rustls.

- OAuth accounts authenticate with SASL `XOAUTH2`, using an access token fetched from the broker for each operation.
  When the provider rotates the refresh token in that exchange, the new one is sealed immediately.
- Self-hosted mailboxes authenticate with their password (`LOGIN` over IMAP, `PLAIN` over SMTP).
- Certificates are verified against the webpki roots, except for the bundled Stalwart, whose certificate is
  self-signed for `localhost`. That traffic stays on the compose network, the same as Postgres and Redis.
- Each exchange has 15 seconds.

The API exposes two checks:

- **Test connection** signs in to IMAP and SMTP and signs out. The outcome is recorded on the account
  (`last_checked_at`, `last_error`) and published, so a failure is data, not an error response.
- **Send test message** sends a plain-text message from the mailbox to itself, proving the send path end to end.

## OAuth broker

Google and Microsoft issue tokens only to registered OAuth apps, and the client secret is needed both to exchange a
code and to refresh a token. A self-hosted Elysium therefore cannot hold an app of its own without its operator
registering one. The broker in `oauth-broker/` is a small, separately deployable service that holds the apps for
everyone: a community deployment serves every self-hosted instance, and anyone can run their own. It is licensed
Apache-2.0 like Elysium, and `oauth-broker/README.md` covers deploying it and registering the apps.

The broker is stateless. It stores no tokens, sessions, or accounts; everything that must survive a browser round
trip is sealed with XChaCha20-Poly1305 under its own key, with the token's purpose as associated data.

### Flow

1. The settings page links to `GET /api/v1/mail/oauth/{gmail|outlook}/start`, a full page navigation.
2. The API generates a `state` and a PKCE verifier, keeps them in Redis for 15 minutes under the `state`, sets the
   `state` as an HTTP-only `SameSite=Lax` cookie scoped to the callback path, and redirects to the broker's
   `/v1/authorize` with `provider`, `redirect_uri` (the API's callback, rebuilt from nginx's `X-Forwarded-Host` and
   `X-Forwarded-Proto`), `state`, and the S256 `code_challenge`.
3. The broker validates the return address and shows an interstitial naming the instance's host in large type,
   with Continue and Cancel. It is served with `frame-ancestors 'none'`, so it cannot be clickjacked.
4. Continue goes to Google or Microsoft with the broker's own sealed state and PKCE challenge. Google is asked for
   `access_type=offline` and `prompt=consent`, Microsoft for `offline_access`, so both issue a refresh token.
5. The provider returns to the broker's `/v1/callback/{provider}`. The broker exchanges the code with its client
   secret, reads the address from the ID token, and redirects to the API's callback with a sealed handoff code that
   holds the refresh token, the address, and the instance's PKCE challenge. It lives two minutes.
6. `GET /api/v1/mail/oauth/callback` requires the cookie to match `state`, takes the pending flow out of Redis
   (`GETDEL`, so a flow completes once), and redeems the handoff at the broker's `POST /v1/redeem` with the PKCE
   verifier. The broker returns the account only when the verifier hashes to the sealed challenge.
7. The API saves or reconnects the account and redirects to `/settings/email?mailConnected=<id>`. Every failure
   redirects with `?mailError=<code>` instead, and the page explains the code.

Every later access token comes from the broker's `POST /v1/refresh`, which relays the refresh token to the
provider with the client secret. Refresh tokens therefore pass through the broker on every use; that is inherent to
a broker holding the secret, and the reason anyone can run their own.

### Protections

- **Return addresses.** HTTPS anywhere; plain HTTP only for `localhost`, loopback, and private network addresses,
  which is how self-hosted instances are usually reached. No credentials or fragments.
- **Consent phishing.** Anyone can start a flow naming any return address. The interstitial shows which instance
  will receive the mailbox before the provider is involved.
- **Intercepted handoff codes.** A code alone is inert: redeeming it needs the PKCE verifier that never leaves the
  instance's backend. Codes expire after two minutes. A stateless broker cannot make them single-use; the
  instance's Redis `GETDEL` makes each flow complete once.
- **Callback forgery.** A callback link crafted in another browser lacks the flow cookie and is refused, so nobody
  can connect their own account into someone else's instance.
- **Swapped tokens.** Provider-state and handoff tokens are sealed under different associated data, so neither
  opens as the other.

### Configuration

| Variable                    | Where         | Effect                                                             |
|-----------------------------|---------------|--------------------------------------------------------------------|
| `OAUTH_BROKER_URL`          | `compose.yml` | The broker as browsers reach it. Empty disables Gmail and Outlook  |
| `OAUTH_BROKER_INTERNAL_URL` | optional      | The broker as the API reaches it, when that differs                |

`GET /api/v1/mail/capabilities` asks the broker which providers it offers, so the settings page can tell an
unconfigured broker from an unreachable one.

## Stalwart

Self-hosted mailboxes live on Stalwart (`stalwartlabs/stalwart:v0.16.22-alpine`), a sidecar in `compose.yml`.

- `stalwart/config.json` names only the datastore (RocksDB under the `stalwart-data` volume). Every other setting,
  including domains, accounts, and listeners, is a JMAP object inside that datastore.
- The API administers it over JMAP at `http://stalwart:8080/jmap/` as the recovery administrator `elysium`, whose
  password is `STALWART_ADMIN_PASSWORD` in `.env`. Compose passes the same value to Stalwart as
  `STALWART_RECOVERY_ADMIN`.
- Creating a mailbox finds or creates its domain (`x:Domain/query`, `x:Domain/set`), gives the server a default
  hostname (`mail.<domain>`) and domain the first time, then creates the user with a generated 256-bit password
  (`x:Account/set`). If saving the row fails, the Stalwart account is destroyed again.
- Deleting a self-hosted mailbox destroys the Stalwart account and its mail before the row is removed.
- A default installation listens on 25, 465, 993, 995, 4190, 443, and 8080. None is published: mail stays on this
  host. Mail between mailboxes on the server is delivered; mail to the internet has no domain, DNS, DKIM, or reverse
  DNS behind it and will be refused or filed as spam.
- Stalwart starts without network access; its web admin UI is not used.

## Tables and routes

`mail_accounts` is described in `docs/database.md`, the routes in `docs/api.md`, and the `mailbox.upserted` and
`mailbox.deleted` events in `docs/realtime.md`.

## Roadmap

- Receiving: an IMAP `IDLE` watcher per active mailbox that ingests new mail, feeding the upcoming mail UI.
- The community broker deployment, then a default `OAUTH_BROKER_URL` pointing at it.
- Google's security assessment for the restricted `https://mail.google.com/` scope, and Microsoft publisher
  verification, so the consent screens stop warning and Gmail's 100-user cap lifts.
- Rate limiting on the broker's public routes.
- Exposing Stalwart beyond this host: a real domain, published SMTP, DKIM keys, DNS records, and a certificate the
  API can verify.
- A dedicated Stalwart administrator for the API in place of the recovery administrator.
