# Storage

Storage locations are the external places Elysium saves files to. They are managed on the Storage settings page
(`/settings/storage`). Nothing writes files to them yet; this doc covers how locations are defined, reached, and
checked.

## Locations

A location has a name, a provider with that provider's own settings, an optional directory, an optional storage
limit, the projects that save files to it, and an access key: Bunny's zone password, or an S3 secret.

| Field               | Meaning                                                                                  |
|---------------------|------------------------------------------------------------------------------------------|
| `provider`          | Where files go: `{ kind, ...settings }`, with `kind` either `bunny` or `s3`               |
| `pathPrefix`        | The directory Elysium writes under, stored without surrounding slashes; empty is the root |
| `storageLimitBytes` | Elysium's own cap on what it stores there; `null` for no limit                           |
| `projects`          | `"*"` for every project, including ones added later, or a list of project ids          |
| access key          | Sealed in `storage_locations.access_key_encrypted`; write-only over HTTP                 |

A location's projects are stored as `storage_locations.all_projects` for `*`, or as rows in
`storage_location_projects`. Choosing `*` removes any links, and deleting a project removes its links. In the form,
`ProjectScopePicker` (`src/components/`) is HeroUI's `Autocomplete` in multiple selection with the picks as tags;
picking `*` replaces the projects picked one by one, and picking a project replaces `*`.

Directories must be plain names separated by single slashes: no `.` or `..` segments, backslashes, or control
characters. The API checks this, and each segment is percent-encoded when a URL is built from it.

Limits are entered and shown in decimal units (1 GB is 10^9 bytes), as storage providers bill them. The largest limit
is 2^53 - 1 bytes, the largest integer a browser holds exactly. A location with no limit takes as much as the
provider allows.

## Providers

`api/src/models/storage_location.rs` stores one row per location with a `kind` column and nullable columns for each
provider's settings, tied to the kind by a check constraint. `StorageProvider` is the typed view of those columns and
the JSON shape clients send and receive. `api/src/storage/mod.rs` holds a client per provider and is the only place
that matches on the kind; routes call `Storage` and never name a provider.

### Bunny Storage

| Setting  | Meaning                                                                      |
|----------|------------------------------------------------------------------------------|
| `zone`   | The storage zone's name: 1 to 64 letters, digits, or hyphens                 |
| `region` | The zone's main region, which decides the endpoint                           |

The access key is the zone's Password, from Access (API / HTTP tab) in Bunny's dashboard, sent as the `AccessKey`
header. The form labels it Password, as Bunny does. The Read-only password lists files but cannot write them, and the
account-wide API key is not used.

Each region has its own endpoint (`api/src/storage/bunny.rs`), and a zone answers only on its own:

| Region         | Endpoint                  |
|----------------|---------------------------|
| `frankfurt`    | `storage.bunnycdn.com`    |
| `london`       | `uk.storage.bunnycdn.com` |
| `new-york`     | `ny.storage.bunnycdn.com` |
| `los-angeles`  | `la.storage.bunnycdn.com` |
| `singapore`    | `sg.storage.bunnycdn.com` |
| `stockholm`    | `se.storage.bunnycdn.com` |
| `sao-paulo`    | `br.storage.bunnycdn.com` |
| `johannesburg` | `jh.storage.bunnycdn.com` |
| `sydney`       | `syd.storage.bunnycdn.com` |

Bunny answers `401` for a wrong password and for a zone that does not exist in the chosen region alike, so the API
reports both as one error that names both causes.

### S3: Amazon S3 and Google Cloud Storage

| Setting       | Meaning                                                                                 |
|---------------|-----------------------------------------------------------------------------------------|
| `service`     | `aws` or `google-cloud`                                                                 |
| `bucket`      | 3 to 222 lowercase letters, digits, dots, hyphens, or underscores                       |
| `region`      | The bucket's AWS Region code, such as `us-east-1`, for `aws`; `null` for `google-cloud` |
| `accessKeyId` | The key's id; the secret half is the location's access key                              |

`api/src/storage/s3.rs` presigns S3 requests (Signature Version 4, through `rusty-s3`) and sends them with Elysium's
shared HTTP client. Endpoints are fixed per service, so no location can point the API at another host:

| Service        | Endpoint                            | Signing region | URL style                                   |
|----------------|-------------------------------------|----------------|---------------------------------------------|
| `aws`          | `https://s3.<region>.amazonaws.com` | The region     | Virtual-hosted; path-style for dotted names |
| `google-cloud` | `https://storage.googleapis.com`    | `auto`         | Path-style                                  |

Dotted bucket names break TLS on virtual-hosted hosts, which is why they switch to path-style.

- **AWS** takes an IAM user's access key. The user needs `s3:ListBucket` on the bucket, and `s3:GetObject`,
  `s3:PutObject`, and `s3:DeleteObject` on its objects.
- **Google Cloud** takes an HMAC key (Cloud Storage Settings, Interoperability) for a service account with Storage
  Object Admin on the bucket.

A wrong key id or secret (`InvalidAccessKeyId`, `SignatureDoesNotMatch`, or Google Cloud's `InvalidSecurity`) is
reported as a refused access key. Any other error, such as `NoSuchBucket` or `AccessDenied`, is reported with the
service's own message and code.

### Keeping the secret when settings change

A Bunny password opens one zone, and an S3 secret belongs to one access key id on one service. Changing the zone, the
service, or the access key id therefore needs the new secret in the same request, and the API answers `400` without
it (`StorageProvider::keeps_access_key_of`). Changing only a Bunny region, an S3 bucket, or an AWS region keeps the
stored secret. The form mirrors this: the secret field turns required as soon as the settings need a new one.

## Testing a location

`POST /api/v1/storage-locations/{id}/test` lists the location's directory with its saved settings and reports how
many entries sit directly inside, and whether there are more. A directory that does not exist yet counts as empty:
Bunny answers `404` or an empty listing, and S3 an empty listing, since directories are only key prefixes. Bunny lists
a whole directory at once; S3 lists up to 1,000 keys, and `hasMore` says the directory holds more. A refusal answers
`502` with the provider's message.

## Roadmap

- Writing files: uploads choose a location, and Elysium refuses a write that would take its usage past a limit it has.
  Usage is counted from Elysium's own writes, since a zone's total size is only reachable with the account API key.
- More S3-compatible services, such as Cloudflare R2 or MinIO, as further `service` values, each with its endpoint
  fixed in code or, for self-hosted ones, validated against the operator's allowed hosts.
- Local and network storage as a third kind.
