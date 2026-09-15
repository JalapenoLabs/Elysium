# Mail

Elysium reads and sends mail through mailboxes connected on the Email settings page (`/settings/email`). This doc
covers the backend: account kinds, the one transport they share, the OAuth broker Gmail and Outlook go through, and
the mail server Elysium runs for self-hosted mailboxes. There is no mail-reading UI; mail will surface through a new
kind of UI later.

## Account kinds

| Kind          | Servers                                                          | Credential                   |
|---------------|------------------------------------------------------------------|------------------------------|
| `gmail`       | `imap.gmail.com:993` TLS, `smtp.gmail.com:465` TLS                | OAuth refresh token          |
| `outlook`     | `outlook.office365.com:993` TLS, `smtp.office365.com:587` STARTTLS | OAuth refresh token          |
| `self_hosted` | Elysium's Stalwart, `stalwart:993` and `stalwart:465`, TLS         | Generated mailbox password   |

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
- Certificates are verified against the webpki roots, except for Elysium's Stalwart, whose certificate is
  self-signed. That traffic stays on the private `elysium-mail` network between the API and Stalwart.
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

## Mail server

Self-hosted mailboxes live on one Stalwart server (`stalwartlabs/stalwart:v0.16.22-alpine`) that the API creates
and runs through Docker. One server hosts any number of domains on one set of listeners, so every domain shares its
ports and its hostname. The API administers it over JMAP at `http://stalwart:8080/jmap/`; every setting, including
domains, accounts, and listeners, is a JMAP object in Stalwart's own datastore.

### Docker

The API reaches Docker only through `docker-proxy` in `compose.yml` (`tecnativa/docker-socket-proxy:v0.5.0`), which
answers the container, image, and volume endpoints and refuses the rest. The proxy is on the internal
`docker-control` network, shared with the API alone. `api/src/mail/stalwart/container.rs` owns everything the API
creates, all labelled `dev.elysium.managed=mail-server`:

| Kind      | Name                      | Holds                                                   |
|-----------|---------------------------|---------------------------------------------------------|
| volume    | `elysium-stalwart-config` | `/etc/stalwart`, the configuration file setup writes    |
| volume    | `elysium-stalwart-data`   | `/var/lib/stalwart`: domains, accounts, keys, and mail  |
| container | `elysium-stalwart`        | The server, `restart: unless-stopped`, no published ports |

The container joins `elysium-mail`, which `compose.yml` defines for nginx, the API, and Stalwart; Stalwart answers
there as `stalwart`. None of the API's resources belong to the compose project, so `docker compose down` leaves the
server running and its data in place (compose reports `elysium-mail` still in use and keeps it for the next `up`),
and `docker compose down --volumes` does not remove them either.

### Creating the server

The Email settings page asks for the first domain and the server's hostname (`mail.<domain>` by default).
`POST /api/v1/mail/server` answers `202` at once and the work runs in the background, each step published as
`mailServer.updated` (`api/src/mail/hosting.rs`):

1. `preparing`: claim the creation, so a second request is refused while it runs.
2. `pulling-image`: pull the pinned image unless Docker has it.
3. `starting`: create the volumes and the container, and start it. With no configuration file, Stalwart starts in
   bootstrap mode and prints a temporary administrator `admin` with a random password to its log.
4. `configuring`: read that password from the container's log, and call `x:Bootstrap/set` with it: the hostname,
   the first domain, a self-signed certificate instead of ACME, DKIM keys, and logging to stdout. Stalwart answers
   with a permanent administrator (`admin@<domain>`), which is sealed into `mail_servers` at once. Stalwart shows
   its password only in that answer, so the database connection is taken before setup starts.
5. `restarting`: restart the container, which is how Stalwart leaves bootstrap mode, and wait for it to accept the
   new administrator. Then trust nginx's PROXY protocol header (see below), which takes another restart.
6. `adding-domain`: record the first domain in `mail_domains` as the default.

With the image already pulled, this takes about four seconds. A failure stops the sequence and the server is
reported `failed` with the reason until creation is started again. A failure after step 4 leaves a working server
whose first domain can be added like any other.

### Keeping it running

At every API start, when a server exists, the API pulls the image if it is gone, starts the container
(recreating it from its volumes if it was removed), and makes sure Stalwart trusts nginx's current address,
restarting it when that changed. `GET /api/v1/mail/server` signs in with the stored administrator on every call,
so a server that stopped answering reads `unreachable`, as does one whose volumes were removed, which no longer
knows the administrator.

### Domains

- Adding a domain creates it in Stalwart (`x:Domain/set`, or finds it with `x:Domain/query`) and records it in
  `mail_domains`. Stalwart generates its DKIM keys (Ed25519 and RSA) within seconds.
- Removing a domain is refused while it has mailboxes, and for the default domain, which Stalwart keeps for itself.
  Otherwise its DKIM keys are destroyed first, since Stalwart refuses to remove a domain they still link to.
- `GET /api/v1/mail/domains/{id}/dns` reads the zone file Stalwart generates for the domain and picks out the
  records delivery and authentication need: MX, SPF (for the domain, and for the hostname on the default domain),
  DKIM, and DMARC. It looks each one up in public DNS (`api/src/mail/dns.rs`, on `hickory-resolver`) and reports
  it `published`, `different` (another value for the same purpose, such as a chosen DMARC policy), `missing`, or
  `unverified` when the lookup itself failed. Whitespace differences, such as a DKIM key split into strings, do not
  count. The full zone file, with optional service discovery records, comes along. Elysium never changes DNS.

### Mailboxes

- A self-hosted mailbox is created on one of the server's domains, with a generated 256-bit password
  (`x:Account/set`). If saving the row fails, the Stalwart account is destroyed again.
- Deleting a self-hosted mailbox destroys the Stalwart account and its mail before the row is removed.
- Domain and mailbox changes answer `503` until a server exists.

### Inbound traffic

nginx is the stack's single ingress, for mail as for the web. It publishes the mail ports on every interface and
passes each connection to Stalwart as raw TCP (`nginx/mail.conf`):

| Port | Protocol                          | TLS                                   |
|------|-----------------------------------|---------------------------------------|
| 25   | SMTP from other mail servers      | STARTTLS, negotiated by Stalwart      |
| 465  | Submission from mail clients      | From the first byte, by Stalwart      |
| 993  | IMAP for mail clients             | From the first byte, by Stalwart      |

`SMTP_PORT`, `SUBMISSIONS_PORT`, and `IMAPS_PORT` move the host side of each, for a host whose ports are taken. nginx
holds no mail certificate: TLS stays end to end with Stalwart, which serves a self-signed certificate today.

nginx resolves `stalwart` per connection, so it starts before any mail server exists, and closes connections while
none does.

Every connection starts with a PROXY protocol header carrying the client's real address. Without it Stalwart would
see nginx as the source of all mail, and SPF, rate limits, and blocklists would judge the wrong address. Stalwart
requires that header from every address it trusts and on every listener, management included, so it trusts exactly
one: nginx's fixed address on `elysium-mail` (`172.29.53.2`, `MAIL_INGRESS_ADDRESS`). The API connects directly and
is never trusted. Containers other than nginx take addresses from `172.29.53.128/25`, so none can claim nginx's.

Everything in front of the host is the operator's to arrange, and whatever they arrange is accepted: a cloud VM with
its public address, a router forwarding the ports, or a tunnel. Pointing a hostname's address record at it, each
domain's records from the DNS check, reverse DNS, and certificates are theirs too. Two limits hold whatever the
setup:

- Tunnels built for HTTP, such as Cloudflare Tunnel, carry web traffic but not inbound SMTP from other mail servers.
  A domain whose MX points through one receives nothing.
- Many residential ISPs block port 25, and many cloud providers, DigitalOcean among them, block outbound port 25.
  Receiving can still work; delivering to other servers then needs a relay, which Elysium does not configure
  today.

At startup Stalwart downloads its web interface and spam-filter data from GitHub. The web interface is not used.

## Tables and routes

`mail_accounts`, `mail_domains`, and `mail_servers` are described in `docs/database.md`, the routes in
`docs/api.md`, and the `mailbox.*`, `mailDomain.*`, and `mailServer.updated` events in `docs/realtime.md`.

## Roadmap

- Receiving: an IMAP `IDLE` watcher per active mailbox that ingests new mail, feeding the upcoming mail UI.
- The community broker deployment, then a default `OAUTH_BROKER_URL` pointing at it.
- Google's security assessment for the restricted `https://mail.google.com/` scope, and Microsoft publisher
  verification, so the consent screens stop warning and Gmail's 100-user cap lifts.
- Rate limiting on the broker's public routes.
- Removing the mail server from the settings page: its container, volumes, domains, and mailboxes.
- A relay (smarthost) for outbound mail, for hosts whose provider blocks port 25.
- Installing a certificate for the server's hostname, replacing the self-signed one.
