# Storage

Storage locations are the external places Elysium saves files to. They are managed on the Storage settings page
(`/settings/storage`). Nothing writes files to them yet; this doc covers how locations are defined, reached, and
checked.

## Locations

A location has a name, a provider with that provider's own settings, an optional directory, an optional storage
limit, the projects that save files to it, and an access key (Bunny calls it the zone's password).

| Field               | Meaning                                                                                  |
|---------------------|------------------------------------------------------------------------------------------|
| `provider`          | Where files go: `{ kind, ...settings }`. Only `bunny` exists today                       |
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

## Testing a location

`POST /api/v1/storage-locations/{id}/test` lists the location's directory with its saved settings and reports how
many entries sit directly inside. A directory that does not exist yet counts as empty, whether Bunny answers `404` or
an empty listing: Bunny creates directories as files are written into them. A refusal answers `502` with the
provider's message.

## Roadmap

- Writing files: uploads choose a location, and Elysium refuses a write that would take its usage past a limit it has.
  Usage is counted from Elysium's own writes, since a zone's total size is only reachable with the account API key.
- S3-compatible buckets (AWS S3, Google Cloud Storage) as a second kind, with endpoint, bucket, and region settings.
- Local and network storage as a third kind.
