# Passwords

Agents sign in to websites and services on their own with logins from a password vault: one Bitwarden or Vaultwarden
account, managed under Settings, Passwords by admins. Elysium runs the official Bitwarden CLI itself, and agents reach
the vault only through Elysium's `elysium_passwords` tools, which read one collection of it and nothing else.

## The account

| Server        | Origin                                   |
|---------------|------------------------------------------|
| Bitwarden US  | `https://vault.bitwarden.com`, fixed     |
| Bitwarden EU  | `https://vault.bitwarden.eu`, fixed      |
| Self-hosted   | Any https URL: Vaultwarden, or Bitwarden's self-hosted edition, with a path if it is served under one |

The CLI signs in with the account's personal API key (`client_id`, which starts with `user.`, and `client_secret`,
from the web vault's Settings, Security, Keys) and unlocks with its master password. An organization's API key cannot
sign the CLI in. A self-hosted server must present a certificate the public roots trust.

Agents read one collection, chosen from the ones the account reaches (`GET /api/v1/password-vault/collections`); no
id is ever typed. Until one is chosen, agents read nothing. A dedicated account for Elysia, a member of the
organization that owns the collection, keeps the rest of anyone's vault out of reach entirely: agents are bounded by
Elysium's check, and the account is bounded by Bitwarden's own permissions.

## Where secrets live

| Secret                      | Where                                                                  |
|-----------------------------|------------------------------------------------------------------------|
| `client_secret`             | `password_vault.client_secret_encrypted`, sealed like every application secret (`docs/secrets.md`) |
| Master password             | `password_vault.master_password_encrypted`, sealed under its own associated data |
| The CLI's state             | The API's data volume, `api-data` at `/var/lib/elysium/bitwarden`      |

The CLI's state is what any Bitwarden client keeps: the access and refresh tokens of its sign-in, its device
identifier, and the vault as downloaded, still encrypted under the account's key. The master password is never written
there; the CLI is unlocked after every start. The volume keeps the CLI signed in as the same device across restarts, so
Bitwarden does not see a new device, or mail its "new device" notice, every time the API restarts or updates.

Neither secret is ever returned. Changing the vault's credentials with a field left blank keeps the stored secret.

## The CLI

The API image carries the CLI's OSS build (`bw-oss-linux`, GPL-3.0, run as a separate program), pinned to a release
at least two weeks old and checked against the digest GitHub publishes (`api/Dockerfile`, stage `bitwarden-cli`). It
runs inside the API's own container, never the agents' workspace, so no agent can read its process, its state, or its
environment.

- **Signing in** is one-shot commands: `bw config server`, then `bw login --apikey` with the key in the environment.
  Secrets travel only in a process's environment or a request body, never on a command line.
- **Everything else** goes through one long-running `bw serve` on the API container's loopback (`127.0.0.1:8087`),
  unlocked once with the master password. Nothing outside the container can reach that address, only Elysium runs
  inside it, and the CLI refuses any request whose `Host` is not loopback or that carries an `Origin`.
- **Connecting** (`PUT /api/v1/password-vault`) signs in, starts `bw serve`, unlocks, syncs, and lists the
  collections before anything is stored, so every stored vault names the account it reaches. A refused key or master
  password answers `400` with Bitwarden's own words, and whatever vault was stored before is brought back.
- **At startup** the API brings the stored vault up. When the CLI is still signed in as the same account on the same
  server, it only unlocks; otherwise it signs in again.
- **Keeping it up.** Every minute the API checks `bw serve` and restarts it if it exited or the vault failed. Every
  five minutes it syncs, so items added in Bitwarden reach agents; admins can sync at once.
- **Removing** the vault stops the CLI, signs it out, and deletes the row. The Bitwarden account and its API key are
  untouched; revoke the key in Bitwarden to be sure it is never used again.

What the CLI is doing is kept in memory, published as `passwordVault.status`, and shown on the settings page:

| State          | Meaning                                                            |
|----------------|--------------------------------------------------------------------|
| `unconfigured` | No vault is stored                                                 |
| `starting`     | Signing in, unlocking, or starting                                 |
| `ready`        | Agents can use it                                                  |
| `failed`       | The last attempt failed, with the reason; it is tried again every minute |

## The agents' tools

`elysium_passwords` (`api/src/tools/passwords.rs`) is granted to every turn of every session while the vault is
ready; a vault that becomes ready mid-turn is granted from the next turn.

| Tool              | Does                                                                 |
|-------------------|----------------------------------------------------------------------|
| `passwords_find`  | Finds logins by the site's URL (Bitwarden's own URI matching), by words in a name, username, or URI, or both. Answers ids, names, usernames, URIs, and whether a login has a one-time code; never a password. At most 25 |
| `passwords_login` | One login's username and password, by id                             |
| `passwords_totp`  | One login's current TOTP code, by id. The seed is never shown        |

Every call reads the chosen collection from the database as it is now, and every item is checked against it before
anything of it leaves the API: a login, not in the trash, in that collection (`api/src/passwords/items.rs`). The
CLI's own filter only decides what is fetched. A login moved out of the collection, or a collection changed mid-turn,
is refused at the next call. Calls are logged by caller (a session or one of Elysia's turns), tool, and item id,
never by what they answered.

Every password and code handed to an agent is remembered for 24 hours (at most 1,000 at once), and every turn's
recorded and published output is scrubbed of them as it is of secret environment variables (`docs/coding.md`). The
agent still holds what it was given; scrubbing keeps it out of history, the event stream, and the browser. The tools'
instructions tell the agent to type secrets straight into the sign-in form and never print, commit, or save them.

## Routes

See `docs/api.md`. Every route is admin-only.

## Roadmap

- Granting the tools to Elysia's own turns once they run on the host runner.
- Custom fields and secure notes, for sites that ask for more than a username and password.
- Agents saving a new login they created into the collection, through a changeset.
- A certificate authority for self-hosted servers behind a private CA.
