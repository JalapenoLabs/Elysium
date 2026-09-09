# Storage & Drives

Elysium's storage layer is a **gateway**, not a sync engine and not a re-implementation
of a filesystem. It presents one unified, keyboard-driven view over many storage
locations, and it is the foundation the service integrations (git, registry, Astraval)
persist onto. See [integrations.md](integrations.md) for how services consume this layer.

## Core model

- **Drives are project-scoped.** Work resources live in **projects**, not loose in the org
  ([tenancy.md](tenancy.md)). A Drive carries a `project_id`; the auto-provisioned default Drive
  lands in the org's default project. Access resolves through the org → project → team
  hierarchy (org-admin bypass, then project/team membership).
- **Capacity = your Drive; quotas are optional policy, not a platform ceiling.** Large-blob
  surfaces (file content, mail bodies/attachments, registry layers, git LFS) are
  Drive-backed, so capacity scales with the bound Drive (point it at a 2 TB bucket and
  there is no artificial "almost full" ceiling). Transactional metadata stays in Postgres.
  An admin *may* set soft quotas for fairness, but Elysium imposes no hard limit.
- **A Drive is a linked storage location.** An organization owns many Drives. A Drive is
  one [OpenDAL](https://opendal.apache.org/) operator rooted verbatim where the org
  pointed it, plus Elysium metadata (name, kind, visibility, default flag, sealed
  credentials). "Drive" is the user-facing term; inside a Drive are ordinary folders and
  files. The S3 word "bucket" survives only inside the S3 driver's config.
- **Gateway, not sync.** Each Drive is a live window onto its provider. Browsing calls the
  provider in real time; nothing is copied. Deleting in Elysium deletes at the provider.
  The provider is always the source of truth. Sync (automated backups, offline copies,
  syncing specific folders to devices) is a later opt-in feature, never the core engine.
- **Org isolation comes from the `org_id` column, not from paths.** Elysium imposes no
  global directory scheme on a Drive; it roots each operator exactly where configured.

### Drive kinds (v1)

| Kind | Provider | OpenDAL service |
|------|----------|-----------------|
| `FS`   | Local filesystem (a directory on the host)         | `services-fs`   |
| `S3`   | Any S3-compatible bucket (AWS, GCS, Bunny, MinIO, Wasabi, B2) | `services-s3` |
| `SFTP` | A path on a remote machine over SSH                | `services-sftp` |

Roadmap, same abstraction, additive: **WebDAV** (Nextcloud/ownCloud/Synology, the next
client to add), then native Azure Blob / native GCS on demand, then the OAuth cloud drives
(Google Drive, OneDrive, SharePoint). The strategic endgame is Elysium *serving* its unified
storage outward as an S3 endpoint and a WebDAV mount, so any OS or tool can mount Elysium.

## The capability contract

Storage **providers** advertise capabilities; storage **consumers** (the user-file gateway,
and every service integration) declare the capabilities each of their storage roles
requires. A binding is legal only when the provider satisfies the requirement. This is what
makes "you would never put git on OneDrive" an enforced rule rather than a guideline.

Three capabilities cover everything in scope:

- **`PLAIN_FILES`**: list/read/write/delete files at paths. Every provider has it.
- **`OBJECT_STORE`**: content-addressed blob put/get/stat by key, strong read-after-write,
  multipart for large blobs. S3 and local FS have it; SFTP/WebDAV/OneDrive/GDrive do not.
- **`POSIX_FS`**: a real local filesystem: locking, rename, mmap, atomic ref updates. Only
  local FS. Git object stores and Astraval metadata physically require this.

| Provider | PLAIN_FILES | OBJECT_STORE | POSIX_FS |
|----------|:-----------:|:------------:|:--------:|
| Local FS | ✓ | ✓ | ✓ |
| S3       | ✓ | ✓ | ✗ |
| SFTP     | ✓ | ✗ | ✗ |
| WebDAV / OneDrive / GDrive *(later)* | ✓ | ✗ | ✗ |

Consumers declare storage **roles**, each with a required capability; one consumer may need
several roles of different capabilities (see [integrations.md](integrations.md)):

| Consumer | Role | Requires | Eligible providers |
|----------|------|----------|--------------------|
| User files (this gateway) | files | PLAIN_FILES | any |
| Docker registry | blobs | OBJECT_STORE | Local FS, S3 |
| Git server | repos | POSIX_FS | Local FS only |
| Mail server | bodies | OBJECT_STORE | Local FS, S3 |
| Astraval | metadata | POSIX_FS | Local FS only |
| Astraval | archive | OBJECT_STORE | Local FS, S3 |
| Runner | workspace | POSIX_FS | Local FS only |

When binding a consumer role, the UI offers only Drives whose advertised capabilities
satisfy the role. Consumer data lives under a reserved prefix (`.elysium/<consumer>/`) that
the file browser hides.

## Access control

A Drive is either **org-wide** or **restricted** (Slack-channel style). The org boundary is
absolute: outsiders never have access, ever; visibility is only ever a choice *within* the org.

- **Org-wide** (`visibility = ORG_WIDE`): every member of the org can see and use it.
- **Restricted** (`visibility = RESTRICTED`): access granted to any mix of teams and
  individual users via `drive_access` rows.
- **Admins and up always have access to every Drive**, enforced in the authz check, never
  stored.

Managing Drives (create/link, edit, set default, delete, set access) requires the
`manage_storage` admin capability, reusing the existing `grants`/`policies`/`authz` machinery.
`manage_storage` is a project-or-org admin action: it is granted to a **project owner or
admin** on the Drive's project (org owners and admins bypass to project Owner through the
org → project → team resolver), or to an ordinary org member holding a **delegated org-level
`manage_storage` grant**. Guests and suspended members never qualify. Reads (`GET /drives`)
are not capability-gated; they return the Drives the caller can access per the rules above.
Access is Drive-level in v1; per-file permissions (Google-Drive-style sharing with stable
file IDs, roles, share links, "shared with me") are a deferred future pillar and are noted
where they touch this design.

## Default Drive and system files

- Exactly one Drive per **project** is the **default** (`is_default`, enforced by a partial
  unique index keyed on `project_id`). Drives are project-scoped, so each project has its own
  default; drag-and-drop uploads land in the active project's default.
- Every project **auto-provisions** a local-FS default Drive when it is created (org
  creation seeds the org's default project, so a fresh org has one immediately), rooted at
  `<data_dir>/<org_id>/<project_id>`, so uploads and avatars work on first run with zero
  admin config. The provider creates the directory on first write. The project can change
  its default to an S3 Drive at any time.
- `data_dir` is the base directory Elysium may write local files to. It resolves from the
  single optional `ELYSIUM_DATA_DIR` env var, defaulting to `/var/lib/elysium` (the mounted
  writable volume in Docker). The resolved per-project root is **persisted in the Drive's
  config**, not re-derived from env at read time, so moving `data_dir` never orphans an
  existing Drive.
- On boot, Elysium **reconciles** the invariant "every project has a default Drive,"
  provisioning the missing ones (idempotent, logged). This backfills projects that predate
  the auto-provisioning wiring.
- System files (avatars, org logos, and each consumer's reserved area) live under
  `.elysium/` inside the project's default Drive, hidden from the file browser (the public
  file API never lists or writes it).

### Avatars and org logos are Drive-backed

Avatars and org logos are stored on a Drive, not just held as opaque external URLs
as follows:

- **Upload.** `PUT /api/v1/users/me/avatar` (any member, active org) and
  `PUT /api/v1/orgs/{id}/logo` (org admin) stream an image into the reserved
  `.elysium/avatars/{user_id}.{ext}` / `.elysium/logos/{org_id}.{ext}` area of the owning
  org's **default Drive** (the default Drive of the org's canonical/oldest project). An
  image content-type allow-list (PNG/JPEG/GIF/WebP; **no SVG**, to avoid inline-script
  surface) and an 8 MiB per-image cap are enforced; the validated body is written whole,
  so a rejected upload never leaves a partial blob.
- **The reference.** The `users.avatar_url` / `orgs.logo_url` column keeps working and
  holds one of two things, told apart by prefix: an **internal reference**
  `elysium://<drive_id>/<key>` (written by an upload), or an **external URL**
  (`http(s)://`, set by the plain profile/branding update). The reference embeds the
  `drive_id`, so it self-locates: an existing avatar keeps resolving even after the
  project changes its default Drive.
- **Serving.** `GET /api/v1/users/{user_id}/avatar` and `GET /api/v1/orgs/{id}/logo`
  stream the blob back through the storage layer with a content type derived from the
  stored extension (an external URL redirects `302`; an empty value is `404`). The
  canonical profile/org reads (`profile::to_user`, `orgs::update`) present an internal
  reference as its served path so a client renders it directly; other list surfaces
  currently expose the raw reference (a follow-up as the frontend adopts these paths).
- **Caveat (unchanged):** changing the default Drive does not migrate existing
  `.elysium/` system blobs. Because each reference embeds its `drive_id`, existing
  avatars/logos keep resolving from the Drive they were written to; only new uploads land
  on the new default.

## Search

**Live, federated, streaming, provider-as-truth. No index.** A persistent Postgres mirror of
file metadata was explicitly rejected: it duplicates the source of truth, bloats at tens of
millions of files, and creates "file no longer exists" staleness footguns.

- A search fans out concurrently to every Drive the user can access. Each Drive searches
  using its provider's best native capability (FS walk; S3 `ListObjects` under prefix +
  filter; SFTP recursive listing; later, Google Drive's native search API for real speed).
- Results **stream per source** so fast Drives light up instantly while a slow one shows a
  spinner the user can watch or cancel; the UI attributes slowness to the specific source.
- Honest tradeoff: filename search across a huge S3 Drive is genuinely slow because S3 has
  no server-side name search. We make it tolerable (parallelism, streaming, cancel, scope
  narrowing) and transparent, not instant.
- v1 is filename/path search. Content/full-text search and unified cross-Drive stable
  identity are part of the deferred per-file-sharing pillar, not this one.

## Transfer

- **Proxy-through-backend, streaming, is the universal fallback.** The client always talks
  to Elysium; the backend streams bytes to/from the OpenDAL operator. This is uniform
  across every provider (the only thing that works for FS and SFTP at all) and keeps one
  client contract.
- Never buffer a whole file in memory; stream both directions and use OpenDAL's multipart
  writer for large objects.
- A configurable **per-file size cap** is a DB-backed org storage setting (default 5 GB),
  edited in the admin UI. Because we stream, the cap is a policy choice, not a memory limit.
- **Presign-if-available (S3), else proxy.** The content routes take an additive opt-in
  `?presign=1`. When the Drive's provider can sign direct URLs (**S3 only**;
  `supports_presign`) and the transfer clears a size threshold (16 MiB; `should_presign`),
  the backend answers with JSON `{ url, method, headers, expires_at }` (a 15-minute
  presigned `GET`/`PUT`) so the bytes flow client↔provider directly. FS/SFTP, sub-threshold
  transfers, or any presign failure fall straight back to the proxy, so presign is a pure
  optimization with no contract change.
- **Atomic replace.** A streamed replace never corrupts the prior object on a failed or
  oversize upload (`stream_to_drive`), per provider:
  - **FS** (`POSIX_FS`) stages to a `.part` temp key in the same directory and renames it
    over the target on success; the FS writer truncates on open, so writing in place would
    corrupt the prior file. A failed replace deletes the temp and leaves the original
    byte-for-byte.
  - **S3** writes directly: PutObject / multipart commit only on close, so an aborted
    replace never touches the prior object (inherently staged).
  - **SFTP** writes directly and has **no** atomic replace (the writer truncates on open);
    a failed replace best-effort deletes the truncated partial. Documented limitation.

## Configuration philosophy

`.env` holds only what is needed to boot and protect the box: the master encryption key
(`ELYSIUM_SECRET_KEY`), the database URL, the bind port, and the single optional
`ELYSIUM_DATA_DIR` (where Elysium may write local files; defaults to `/var/lib/elysium`, the
mounted Docker volume). Everything operational is configured in-app and stored in the DB: the
upload cap (`org_storage_settings.max_upload_bytes`, no env var governs it), Drive
definitions, and each Drive's persisted root. "Set an encryption password and launch,
configure the rest in the app" is the north star.

## Encryption at rest

Three layers, three different calls:

1. **Credentials: always, app-level.** A Drive's provider secret (S3 access/secret
   keys, SFTP private key) is sealed with `crypto.rs` (XChaCha20-Poly1305 under
   `ELYSIUM_SECRET_KEY`), the same pattern as TOTP secrets, bound to the Drive's
   `project_id`. Opened, the blob is per-kind JSON: `{access_key_id, secret_access_key}`
   for S3, `{kind: private_key, pem}` for SFTP. Never returned to the client; the API
   exposes only "credential set / not set." **SFTP is key-based only**, and this is a
   deliberate, investigated limitation, not an oversight: OpenDAL 0.57's `services-sftp`
   runs on the `openssh` crate, whose builder exposes no `password` method (verified), so
   there is no clean way to pass a password through the operator. The alternatives each
   break an invariant, a libssh2 crate links system OpenSSL (violating no-OpenSSL), and a
   `russh` client would fork the "one OpenDAL operator per Drive" model, so a password
   credential is rejected with a clear error rather than hacked in. It unblocks if OpenDAL
   gains SFTP password auth upstream, or Elysium adopts a `russh`-backed `Access`
   implementation (pure-Rust, rustls, no OpenSSL).
2. **In transit: inherent per provider.** TLS for S3/WebDAV, SSH for SFTP, on-box for FS.
3. **File bodies: infrastructure, not app-level, for now.** No app-level body encryption:
   it would break native provider use, presign, previews, and content search. Rely on disk
   encryption (LUKS/BitLocker) for FS and provider SSE (SSE-S3/SSE-KMS, exposed as
   non-secret S3 config) for S3. A zero-knowledge "Encrypted Drive" (envelope encryption on
   the managed local store, accepting the loss of native usability and previews) is a
   deliberate later opt-in.

## Proto & API surface

Protobuf for all metadata/control; raw octet streams for file bytes. Package
`elysium.storage.v1`. Endpoints under `/api/v1`:

- Drive management (admin, `manage_storage`): `GET/POST /drives`, `PATCH/DELETE /drives/{id}`,
  `POST /drives/{id}/test` (validate a connection before saving), `PUT /drives/{id}/access`,
  `POST /drives/{id}/default`.
- Files within a Drive (path as URL-encoded `?path=`): `GET /drives/{id}/files` (list →
  `Listing`), `GET|PUT /drives/{id}/content` (download/upload, raw stream),
  `POST /drives/{id}/folders`, `POST /drives/{id}/move`, `DELETE /drives/{id}/files`.
- Search: `POST /search` streaming length-delimited `SearchFrame` messages over one chunked
  response, one frame per source as it finishes; cancel by dropping the connection.
