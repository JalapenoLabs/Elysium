# Studio

Studio is where agents make single assets: a 3D model built in Blender, a render, a 2D image. A Studio item is one
asset and the conversation that shapes it. It is not tied to a git repository; the Coding area is where
multi-repository, long-lived work with pull requests happens.

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
- Its instructions are Studio's (`api/src/studio/instructions.rs`), written into the agent's `AGENTS.md` below the
  satellite's header, which no host can override and which already says deliverables go in `artifacts/`.
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
2. Records it without reading it when the item already stores those bytes under another path or version: objects are
   named by content, so the kept object is never uploaded over.
3. Checks the location's storage limit against the bytes Studio has recorded there.
4. Streams the file out of the workspace (`read_file("artifacts/<path>")`) into the item's storage location at
   `studio/<itemId>/<sha256>.<extension>`, hashing it as it passes. Nothing is held in memory. Bytes that no longer
   match the announcement are removed and not recorded; the turn that changed them announces them again.
5. Records a `studio_assets` row, then publishes `studioAsset.created`, so no client hears of a file before it can be
   fetched.

A file that cannot be kept (over the location's limit, refused by the provider) is recorded on the item as its
`pullError`, which the item page shows; the next file that is kept clears it.

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
URL or credential reaches the browser. `?download=true` adds `Content-Disposition: attachment` with the artifact's
name. Files are addressed by content, so responses are cached as immutable. An agent wrote the bytes, so they are served
under the API's own policy, `default-src 'none'; frame-ancestors 'none'; sandbox`, with `nosniff` (see
`docs/security.md`): an SVG opened directly runs no script.

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

Every prompt is sent as `multipart/form-data` to `POST /api/v1/studio-items/{id}/turns`
(`api/src/routes/v1/studio_items/send_turn.rs`): `prompt`, `satelliteId` (where a continued item runs), and for a
drawing `annotated` (PNG), `capture` (PNG), `sourceAssetId`, and `cameraOrbit`. A drawn prompt:

1. Keeps both images in the item's storage location at `studio/<itemId>/feedback/<feedbackId>/`, within the location's
   limit, and records a `studio_feedback` row with the prompt, the source asset, the camera orbit, and who sent it.
2. Uploads the drawing into the workspace at `feedback/<feedbackId>/annotated.png` and starts the turn with it as an
   Arsox turn attachment. The satellite hands images to the harness as native image input, so the agent sees exactly
   what was drawn. The row then names the turn it started; a turn the satellite refuses takes the row and its images
   with it.

The drawing is at most 3.75 MiB, the limit Arsox and the model APIs set for an attached image, so the frontend
downscales a large capture before it draws. The clean capture is only kept, never sent, and may be 12 MiB. A prompt
without a drawing attaches nothing. The answer is `{ session, turn, feedback, continued }`.

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
3. When it cannot (no transcript, the other family, or an import the satellite refuses), the first turn carries a
   brief ahead of the prompt instead: the original prompt, the latest 20 feedback prompts, and the thumbnail as an
   attachment.

The session records which happened as `continuation` (`imported` or `brief`; `null` for a session that continued
nothing), and the conversation view says so at the divider. Copying files back is all or nothing: a provider or
satellite that refuses a file discards the new session rather than leave a workspace missing some of the item's work.

An item has at most one live session. Continuing takes a Redis lock per item for its duration, so a second prompt sent
meanwhile answers `409` instead of opening a second thread. While the latest thread is live, `satelliteId` is ignored
and the turn runs there; a latest thread whose satellite cannot be reached answers `502` rather than continuing, since
that thread may still be running. The harness session is exported after every turn the satellite reports complete
(`api/src/studio/transcripts.rs`) and kept sealed; a failed export is logged and only costs a later continue its
import.

The conversation view reads the item's sessions in order and shows a divider where one continued into the next. Each
session in an item's response carries `continuation`: `imported` or `brief` for how it carried on, and `null` for the
item's first session. The divider names it, and says only that a new thread began when it is absent.

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

### Grid

`/studio` (`StudioPage`) is tiles only (`StudioItemTile`):

- **Tile.** The thumbnail, title, live thread state, and image and model counts. The state chip shows only once the
  item's latest thread has a known state (`getTileThreadState`): an item with no session, or a thread not polled yet,
  has none. The thumbnail is the API's choice (`thumbnailAssetId`); without one, a working placeholder shows while the
  item's thread runs or waits, else an empty frame (`getTileThumbnail` in `studioListing.ts`).
- **Filters.** A project (or no project) and a Deleted items switch, kept in the address (`?project=`,
  `?deleted=true`), so a filtered grid survives opening an item. Deleted tiles offer Restore.
- **New item** (`CreateStudioItemForm`). The prompt, an optional title and project, a storage location, and a
  satellite. The location list follows the project as the API does, and starts on Studio's default when it is among
  them (`studioStorage.ts`).
- **No storage.** With no storage location at all, the page shows `StudioNoStorage`, which links to Settings,
  Storage, in place of the grid.

### Item page

`/studio/:itemId` (`StudioItemPage`, then `StudioItemWorkspace`) takes the whole content area.

- **Layout.** Two columns split by `react-resizable-panels` (`StudioSplit`): the stage and the conversation.
  - A three-way control in the header (`StudioLayoutControl`) shows Both, Preview, or Chat. One mode is always chosen,
    so both columns can never be hidden.
  - Neither column narrows below 20% of the width. Dragging one well past that closes it and switches to the other's
    mode.
  - The split and mode are saved per browser under `elysium.studio.layout.v1`. Anything unreadable falls back to
    Both at 60% (`studioLayout.ts`).
  - Narrower than 900 pixels, the columns stack and the same control switches between them. Hidden columns stay
    mounted, so the model stays loaded.
- **Stage** (`StudioStage`). The image or the 3D viewer, with the file's path above and two actions: Pin as thumbnail
  for an image, and Annotate.
  - The filmstrip (`StudioFilmstrip`) lists every model and image the item holds, then the versions of the one shown,
    newest first, to compare turns. The stage follows the newest version until an older one is picked.
  - Downloads list the newest version of every file, grouped by stem (`groupStudioAssets` in `studioAssets.ts`).
  - The stage opens on a model the viewer can show, else the image the tile shows, else the newest image, else a
    model without a `.glb`, which shows its downloads.
- **Conversation** (`StudioConversation`). One scrolling column of the item's sessions, oldest first, each a
  `StudioSessionTimeline` built on the Coding page's `TimelineEventList`. A divider sits between sessions. A drawn
  prompt shows its drawing under the prompt, matched by `turnId`.
- **Composer** (`StudioComposer`). It wraps the Coding page's `PromptComposer` but never closes, since a prompt to an
  ended thread is how an item continues. It sends through `sendStudioTurn`. Without a live thread, a notice and a
  satellite picker sit on a row above the field, and the picker wraps under the notice in a narrow column
  (`getStudioThreadStatus` in `studioContinuation.ts` mirrors the API's rule):
  - The latest thread ended on a satellite that still exists: the picker offers Previous satellite, the default, which
    leaves the choice to the API.
  - The latest session's satellite was deleted, or the item has never run (its creation failed): there is no previous
    satellite, so the picker has no such option and Send stays disabled, saying why, until a satellite is chosen.
  - No satellite is active: the picker is disabled and Send says so.
- **Deleted items.** A deleted item is read-only under a banner with Restore. A pull error shows as a banner too.
- **Delete** (`StudioItemActions`). It confirms through `useConfirm`, whose message (`DeleteStudioItemMessage`) holds
  the unchecked permanent box. A soft delete rereads the item to show it deleted; a permanent one returns to the grid.

### 3D viewer

The viewer is `@google/model-viewer` (`StudioModelViewer`), imported lazily so three.js loads with the first model.

- **Off-origin loads.** It loads nothing from another origin: `ModelViewerElement.mapURLs` passes every URL its
  loaders ask for through `keepToOwnOrigin` (`modelViewerUrls.ts`), which refuses any URL off the app's origin.
- **Decoders are off.** model-viewer fetches its Draco and KTX2 decoders from Google's CDN, and Meshopt's only when a
  location is set. All three run as blob workers with WebAssembly, which the web app's policy does not allow (no
  `worker-src`, no `'wasm-unsafe-eval'`), so bundling them locally would not make them run either.
  - Elysium's export (`api/src/blender/export_glb.py`) never compresses, so its models need no decoder.
  - A compressed `.glb` an agent writes itself fails to preview, in production and development alike, and offers its
    downloads instead.
- **Textures.** three.js unpacks a `.glb`'s embedded textures into blob URLs and reads them with `fetch`, which the
  production policy's `connect-src 'self'` refuses (checked in Chromium). A textured model therefore renders without
  its textures in production; the development stack sends no policy and shows them. See `docs/security.md`.

### Annotate

`AnnotateModal` draws over a view at its full resolution.

- **Source.** An image opens as it is. A model's view is frozen first with `toBlob()`, and `getCameraOrbit()` is
  recorded.
- **Drawing.** Strokes are `perfect-freehand` outlines (or an arrow) in image pixels (`annotation.ts`), painted by
  `drawAnnotation.ts`. The pen has three colors, the theme's danger, warning, and accent, plus undo and clear.
- **Sending.** The canvas itself is the flattened PNG. It goes to `sendStudioTurn` with the prompt, the source asset,
  the orbit, and, for a model, the clean capture.

### Settings

Settings, Storage has a Studio default column whose switch (`StudioDefaultSwitch`) sets `isStudioDefault`.

## Roadmap

- Showing textured models in production, which needs the web app's policy to allow `blob:` in `connect-src`
  (`docs/security.md`).
