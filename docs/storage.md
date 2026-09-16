# Storage

Storage locations are the external places Elysium saves files to. They are managed on the Storage settings page
(`/settings/storage`). This doc covers how locations are defined, reached, and checked, and the file operations the API
performs on them. No route or agent calls the file operations yet; the storage gateway for coding agents is on the
roadmap.

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

## File operations

`Storage` (`api/src/storage/mod.rs`) lists, describes, downloads, uploads, and deletes files in a location. Each
operation takes the location, its decrypted access key, and a path relative to the location's directory; entries come
back named the same way, so a listed path can be passed straight to another operation.

| Operation  | Returns                                        | Bunny request             | S3 request                         |
|------------|------------------------------------------------|---------------------------|------------------------------------|
| `list`     | A page of entries directly inside a directory  | `GET` with trailing slash | `ListObjectsV2`, delimiter `/`     |
| `stat`     | One entry, or nothing                          | `DESCRIBE`                | `HeadObject`, then a 1-key listing |
| `download` | The file as a stream, with its length and type | `GET`                     | `GetObject`                        |
| `upload`   | The written file's entry                       | `PUT`                     | `PutObject`                        |
| `delete`   | Nothing                                        | `DESCRIBE`, then `DELETE` | `HeadObject`, then `DeleteObject`  |

An entry has a `path`, a `kind` (`file` or `directory`), `sizeBytes` for a file, and `modifiedAt` when the provider
keeps one. S3 keeps no times for directories. An upload's entry is built from what was sent, with no `modifiedAt`,
since neither provider describes the file in its answer.

### Paths

A path is plain names separated by single slashes: no leading, trailing, or doubled slash, no `.` or `..` segment, and
no backslash or control character (`api/src/storage/path.rs`). The empty path is the location's directory, which `list`
accepts and the other operations refuse. The path joined to the location's directory must be 1,024 bytes or fewer,
S3's key limit, applied to Bunny too so a path valid on one location is valid on any. Bunny URLs percent-encode each
segment; S3 keys are signed by `rusty-s3`.

### Listing

Bunny lists a whole directory in one page and hands out no cursor, so a cursor sent for a Bunny location is refused.
S3 pages hold up to 1,000 entries, and `nextCursor` is the continuation token for the next. A directory that does not
exist yet is empty on both. An S3 key equal to the directory itself (a marker some tools create) is not listed.

On S3, `stat` reports a key with no object as a directory when any key sits beneath it.

### Uploads

A file is sent in one request of at most 5 GiB (`MAX_UPLOAD_BYTES`), S3's ceiling for a single `PutObject`, applied to
every provider. A larger file is refused before any request. The caller declares the length, which is sent as
`Content-Length` so the body goes out unchunked (S3 refuses chunked uploads); a body longer or shorter than declared
ends the upload with an error, and the provider keeps nothing partial.

Bunny's upload labels the body `application/octet-stream` and sends the file's type in `Override-Content-Type`, as
Bunny's own SDK does, since Bunny answers `401` to a body it does not take for raw binary. S3 takes the type in
`Content-Type`, sent unsigned alongside the presigned URL.

### Deleting

Only files are deleted. The path is described first: Bunny deletes a directory with everything inside it, and on S3 a
directory is a prefix no single delete removes, so a directory is refused. Deleting a file that is not there
succeeds, so a retried delete is harmless: S3 answers `204` either way, and a `404` from Bunny or Google Cloud's
`NoSuchKey` counts as deleted.

### Streaming and timeouts

File bodies stream in both directions and are never held in memory whole (`api/src/storage/transfer.rs`). A fixed
deadline would cut off a large file on a slow link, so transfers are bounded by progress instead: the provider must
start answering within 60 seconds, and a body that moves no bytes for 60 seconds, in either direction, ends as
stalled. Calls that move no file (listing, describing, deleting) keep a 60-second limit on the whole request.

### Errors

| `StorageError` | Meaning                                                                       | HTTP  |
|----------------|-------------------------------------------------------------------------------|-------|
| `Invalid`      | A path not plain, a Bunny cursor, a bad upload size, or a directory to delete | `400` |
| `NotFound`     | Nothing to download: Bunny's `404`, S3's `NoSuchKey`                          | `404` |
| `Unauthorized` | The provider refused the password or key, with what to check                  | `502` |
| `Refused`      | The provider refused otherwise or could not be reached, with its message      | `502` |

S3 `HEAD` answers carry no error document, so a `403` from `stat` (and from `delete`, which describes first) cannot
tell a wrong key from a missing permission, and is reported as a refused key naming both. A missing bucket is
`Refused` with the service's message, not `NotFound`. Presigned URLs carry their signature, so no error includes a
request's URL.

## Roadmap

- A storage gateway for coding agents: an MCP server that lists, uploads, downloads, and deletes files in the locations
  linked to a session's project, through these operations.
- Usage limits: Elysium refuses a write that would take its usage past a location's limit. Usage is counted from
  Elysium's own writes, since a zone's total size is only reachable with the account API key.
- Multipart uploads for files over 5 GiB: S3's `CreateMultipartUpload` and part uploads, and chunked uploads for Bunny
  if it documents a ceiling.
- More S3-compatible services, such as Cloudflare R2 or MinIO, as further `service` values, each with its endpoint
  fixed in code or, for self-hosted ones, validated against the operator's allowed hosts.
- Local and network storage as a third kind.
