# Studio

Studio is where agents make single assets: a 3D model built in Blender, a render, a 2D image. A Studio item is one
asset and the conversation that shapes it. It is not tied to a git repository; the Coding area is where multi-repository,
long-lived work with pull requests happens.

| Area   | Unit                     | Workspace                   | Output                                |
|--------|--------------------------|-----------------------------|---------------------------------------|
| Coding | A coding session         | Cloned repositories         | Commits and pull requests             |
| Studio | A Studio item (one asset) | No repositories; `artifacts/` | Renders, models, and images Elysium keeps |

The page is `/studio`, a grid of tiles, and `/studio/:itemId`, one item. The API is `/api/v1/studio-items`.

## Items and sessions

A Studio item runs on coding sessions like any other: the same satellites, credentials, environment, Blender, event
stream, and conversation view. `coding_sessions.studio_item_id` marks a session as Studio's; a session without one is a
Coding session. The Coding page lists only Coding sessions, and Studio shows its sessions inside their items.

An item has many sessions over its life, at most one of them live. Its first session starts when the item is created.
When that thread expires or its satellite goes away, the next prompt opens a new session on the same item (see
[Continuing](#continuing)), so the item reads as one conversation however many threads it took.

| Field                 | Meaning                                                                        |
|-----------------------|--------------------------------------------------------------------------------|
| `title`               | From the form, or the start of the first prompt                                |
| `prompt`              | The first prompt, kept for Continue's brief                                    |
| `projectId`           | Optional. With a project, the agent gets `elysium_work` scoped to it           |
| `storageLocationId`   | Required. Where every file of the item is kept                                 |
| `thumbnailAssetId`    | The image pinned as the tile's thumbnail, or `null` for the default            |
| `deletedAt`           | Set by a soft delete                                                           |

A Studio session's `project_id` is its item's, and may be `null`; a Coding session always has one. A check constraint
holds both rules.

## Threads

A Studio thread is opened with Elysium's usual policy (idle TTL, cost and wall clock ceilings, credential stack,
environment, Blender services; see `docs/coding.md`) and differs in four ways:

- It clones no repositories and gets no GitHub token.
- Its instructions are Studio's (`api/src/studio/instructions.rs`), a fixed block ahead of the satellite's own.
- It declares a turn-end hook, `/opt/elysium/bin/export-glb` (see [Model export](#model-export)).
- It declares `elysium_work` only with a project, and `elysium_storage` when the item's project, or with no project any
  location for every project, has a location.

### What agents are told

The instructions are a code constant, like the rest of Elysium's thread policy:

- One asset per item. Deliverables go in `artifacts/`; scratch work stays out of it.
- 3D work uses Blender through the `blender` MCP server. The source is saved as `artifacts/<stem>.blend`. Elysium
  exports the `.glb` itself, so the agent never does.
- After a turn that changes what the asset looks like, the agent renders four views to
  `artifacts/renders/<stem>-<view>.png`: `hero` (three-quarter), `front`, `side`, and `top`, at 1024 by 1024, on a
  neutral studio backdrop with three-point lighting. A fixed set lets turns be compared side by side.
- 2D work writes `artifacts/<stem>.png`, and `artifacts/<stem>.svg` when vector suits it, plus
  `artifacts/<stem>-hero.png` when a presentation mockup helps.
- An attached image is the user's drawing over a render or view: strokes mark what to change, and the prompt says how.

## Assets

An agent's deliverables reach Elysium through Arsox artifacts. At the end of every turn the satellite runs the thread's
turn-end hooks, then scans `artifacts/` and emits `ArtifactCreated` for every file that is new or whose content changed.
The session watcher hands each one to `api/src/studio/artifacts.rs`, which:

1. Skips it when the item already holds that path with that sha256, so a re-announced or seeded file costs nothing.
2. Checks the location's storage limit against the bytes Studio has recorded there.
3. Streams the file out of the workspace (`read_file("artifacts/<path>")`) into the item's storage location at
   `studio/<itemId>/<sha256>.<extension>`. Nothing is held in memory.
4. Records a `studio_assets` row, then publishes `studioAsset.created`, so no client hears of a file before it can be
   fetched.

Events can be missed (a dropped stream the satellite cannot resume), so the watcher also reconciles: whenever it
attaches to a Studio thread, it lists the thread's artifacts and pulls any it has not recorded.

Every version is kept. A render overwritten by the next turn arrives as a new asset with the same path, which is what
lets the filmstrip compare turns.

| Kind    | Files                                                     | Shown as                          |
|---------|-----------------------------------------------------------|-----------------------------------|
| `image` | PNG, JPEG, WEBP, GIF, SVG                                 | In the stage; can be the thumbnail |
| `model` | `.glb`, `.gltf`, `.blend`, `.fbx`, `.obj`, `.stl`, `.usd*` | In the 3D viewer when a `.glb` exists |
| `file`  | Anything else                                             | A download                         |

Files of one model share a stem: `banana.blend` and `banana.glb` are one model with two downloads, and its `.glb`
drives the viewer. Grouping is by path without its extension, done in the frontend.

### Serving files

`GET /api/v1/studio-items/{id}/assets/{assetId}/content` streams the file from the storage location, so no provider
URL or credential reaches the browser. `?download=1` adds `Content-Disposition: attachment` with the artifact's name.
Files are addressed by content, so responses are cached as immutable. Every response carries
`Content-Security-Policy: sandbox; default-src 'none'; style-src 'unsafe-inline'` and `nosniff`, because an agent
wrote the bytes: an SVG opened directly runs no script.

### The thumbnail

The tile shows the pinned image when one is pinned. Otherwise it shows the newest image whose name ends in `-hero`,
then the newest image of any name, then a placeholder while the first turn runs. The rule is a pure function in
`api/src/studio/thumbnail.rs`.

## Model export

Elysium, not the agent, turns every `.blend` into a `.glb`, deterministically, inside the workspace so textures the
file references by path resolve. Elysium's setup script (`api/src/blender/setup.sh`) installs
`/opt/elysium/bin/export-glb` and its Blender script beside Blender; every Studio thread declares it as a turn-end hook.
After each turn it exports each `artifacts/**/*.blend` newer than its `.glb` with Blender's glTF exporter (binary,
modifiers applied, +Y up). The `.glb` then arrives as an artifact like any other.

A failed export is a hook failure: the satellite reports it as a degraded incident, the turn stands, and the `.blend`
still arrives. The model shows without a preview.

## Feedback

Feedback is a prompt, optionally with a drawing. Annotate on an image opens it under a drawing layer; in the 3D viewer
it freezes the current view first (`model-viewer`'s `toBlob()`) and records the camera orbit. The user draws with a pen
in three colors or an arrow, with undo and clear, and writes what to change.

A drawn prompt is sent as `multipart/form-data` to `POST /api/v1/studio-items/{id}/turns`:

1. The API uploads the flattened drawing into the workspace at `feedback/<n>/annotated.png` and starts the turn with it
   as an Arsox turn attachment. The satellite hands images to the harness as native image input, so the agent sees
   exactly what was drawn.
2. It keeps both the drawing and the clean capture in the item's storage location, and records a `studio_feedback` row
   with the prompt, the source asset, the camera orbit, and the turn it started.

A prompt without a drawing is plain text and attaches nothing.

## Continuing

Satellites own no data. Elysium keeps what a thread needs to carry on, and hands it to a new thread when the old one is
gone, so an item resumed a year later picks up where it stopped.

| Kept                  | Where                              | Captured                                  |
|-----------------------|------------------------------------|-------------------------------------------|
| Every thread event    | Postgres `session_events`          | As the session watcher receives it        |
| The harness session   | Postgres `session_transcripts`, sealed | After every turn                      |
| Every artifact        | The item's storage location        | At the end of every turn                  |

A prompt to an item whose latest thread has ended (expired, destroyed, or on a satellite that was deleted) opens a new
session on the same item, on the chosen satellite or else the previous one:

1. The latest version of every artifact path is copied from storage back into the new workspace's `artifacts/`.
2. When the new thread runs the same harness family as the stored transcript, the transcript is imported
   (`import_session`), and the harness resumes its own conversation with full context.
3. When it cannot (no transcript, or the credential stack now picks the other family), the first turn carries a brief
   instead: the original prompt, the feedback given so far, and the thumbnail as an attachment. The conversation view
   says which happened.

The conversation view reads the item's sessions in order and shows a divider where one continued into the next.

## Deleting

One Delete action opens a confirmation with an unchecked box: "Also permanently delete its files, conversation, and
agent memory".

- Unchecked is a soft delete. The item leaves the grid and appears under the Deleted filter, where it can be restored.
  Its live thread is destroyed at once, which stops cost and frees the satellite's disk; everything Elysium keeps
  stays.
- Checked deletes permanently: every file under `studio/<itemId>/` in the storage location first, then the item, its
  assets, feedback, and sessions with their events and transcripts. When the provider refuses a file, the item stays
  and the response names the failure, so no rows vanish while their files linger in a bucket.

A storage location cannot be deleted while items keep files in it, as a project cannot while sessions belong to it.

## Storage locations

The New item form defaults to the location marked as Studio's default under Settings, Storage
(`storage_locations.is_studio_default`, at most one). With a project, the choice is limited to locations for that
project or for every project; without one, to locations for every project. With no location at all, Studio shows an
empty state that links to Settings, Storage.

## Realtime

| Type                    | `data`          | Sent when                                              |
|-------------------------|-----------------|--------------------------------------------------------|
| `studioItem.upserted`   | `StudioItem`    | An item was created, renamed, pinned, continued, or restored |
| `studioItem.deleted`    | `{ id }`        | An item was deleted, softly or for good                 |
| `studioAsset.created`   | `StudioAsset`   | A file was pulled from a workspace and stored           |
| `studioFeedback.created`| `StudioFeedback`| A drawn prompt was sent                                 |

Studio sessions publish `session.upserted` and `session.event` like any session.

## Frontend

`frontend/src/pages/Studio/` holds the grid and the item page.

- The grid is tiles only: the thumbnail, title, live thread state, and model and image counts, filtered by project and
  by live or deleted.
- The item page is two columns split by `react-resizable-panels`: the stage (viewer or image, the filmstrip, Annotate)
  and the conversation (`ConversationTimeline` and `PromptComposer` from the Coding page). A three-way control in the
  header shows Both, Preview, or Chat, so both columns can never be hidden; dragging a column past its collapse point
  switches to the matching mode. The split and mode are saved per browser under `elysium.studio.layout.v1`. Narrow
  screens stack the columns and the same control switches between them.
- The 3D viewer is `@google/model-viewer`. Drawing uses a canvas with `perfect-freehand` strokes.
