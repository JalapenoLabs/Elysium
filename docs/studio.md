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

A Studio item runs on a coding session like any other: the same runner, credentials, environment, Blender, event
stream, and conversation view (see `docs/coding.md`). `coding_sessions.studio_item_id` marks a session as Studio's; a
session without one is a Coding session. The Coding page lists only Coding sessions, and Studio shows its sessions
inside their items.

An item has one session, opened when the item is created, and its directory in the workspace keeps the item's work
between turns. An item whose session could not be opened gets one with its next prompt. An item with several sessions
reads them as one conversation, oldest first.

| Field                 | Meaning                                                                        |
|-----------------------|--------------------------------------------------------------------------------|
| `title`               | From the form, or the start of the first prompt                                |
| `prompt`              | The first prompt                                                               |
| `projectId`           | Optional. With a project, the agent gets `elysium_work` scoped to it           |
| `storageLocationId`   | Required. Where every file of the item is kept                                 |
| `thumbnailAssetId`    | The image pinned as the tile's thumbnail, or `null` for the default            |
| `deletedAt`           | Set by a soft delete                                                           |

A Studio session's `project_id` is its item's, and may be `null`; a Coding session always has one. A check constraint
holds both rules.

## Sessions

A Studio session runs like a Coding session (see `docs/coding.md`) and differs in four ways:

- It clones no repositories and gets no GitHub token.
- Every turn carries Studio's instructions (`api/src/studio/instructions.rs`) as an addition to the agent's system
  prompt.
- After every turn, Elysium exports its models and collects its deliverables (see [Assets](#assets)).
- It gets `elysium_work` only with a project, and `elysium_storage` when the item's project, or with no project any
  location for every project, has a location.

### What agents are told

The instructions are a code constant, like the rest of Elysium's session policy:

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

An agent's deliverables are the files in its session's `artifacts/` directory. After every turn,
`api/src/studio/artifacts.rs` exports the item's models (see [Model export](#model-export)), lists `artifacts/` with
each file's size and sha256, and for every file:

1. Skips it when the item already holds that path with that sha256, so a re-announced or seeded file costs nothing.
2. Records it without reading it when the item already stores those bytes under another path or version: objects are
   named by content, so the kept object is never uploaded over.
3. Reserves the file's bytes in a location with a storage limit (`api/src/models/studio_reservation.rs`): under a lock
   per location it counts what Studio has recorded there plus what uploads in flight have reserved, so concurrent
   uploads cannot pass the limit together. The reservation is released once the file is recorded or refused; one a
   crash left behind stops counting after an hour.
4. Streams the file out of the workspace (`docker exec` as `agent`, confined to the session's directory) into the
   item's storage location at `studio/<itemId>/<sha256>.<extension>`, hashing it as it passes. Nothing is held in
   memory. Bytes that no longer match the listing are removed and not recorded; the next turn's listing finds them.
5. Records a `studio_assets` row, then publishes `studioAsset.created`, so no client hears of a file before it can be
   fetched.

A file that cannot be kept (over the location's limit, refused by the provider) is recorded on the item as its
`pullError`, which the item page shows; the next file that is kept clears it.

Every turn lists the whole directory, so a file a failed turn left behind is kept after the next one.

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
name. Files are addressed by content, so responses are cached as immutable. An agent wrote the bytes, so they are
served under the API's own policy, `default-src 'none'; frame-ancestors 'none'; sandbox allow-downloads`, with
`nosniff` (see `docs/security.md`): an SVG opened directly runs no script, and its download still works.

### The thumbnail

The tile shows the pinned image when one is pinned. Otherwise it shows the newest image whose name ends in `-hero`,
then the newest image of any name, then a placeholder while the first turn runs. The rule is a pure function in
`api/src/studio/thumbnail.rs`.

## Model export

Elysium, not the agent, turns every `.blend` into a `.glb`, deterministically, inside the workspace so textures the
file references by path resolve. The workspace image carries `/opt/elysium/bin/export-glb` and its Blender script
(`workspace/blender/`). After each Studio turn it exports each `artifacts/**/*.blend` newer than its `.glb` with
Blender's glTF exporter (binary, modifiers applied, +Y up). The `.glb` is then collected like any other file.

A failed export is logged; the turn stands and the `.blend` is still kept. The model shows without a preview.

## Feedback

Feedback is a prompt, optionally with a drawing. Annotate on an image opens it under a drawing layer; in the 3D viewer
it freezes the current view first (`model-viewer`'s `toBlob()`) and records the camera orbit. The user draws with a pen
in three colors or an arrow, with undo and clear, and writes what to change.

Every prompt is sent as `multipart/form-data` to `POST /api/v1/studio-items/{id}/turns`
(`api/src/routes/v1/studio_items/send_turn.rs`): `prompt`, and for a drawing `annotated` (PNG), `capture` (PNG),
`sourceAssetId`, and `cameraOrbit`. A drawn prompt:

1. Keeps both images in the item's storage location at `studio/<itemId>/feedback/<feedbackId>/`, reserving their bytes
   within the location's limit as a delivered file's are, and records a `studio_feedback` row with the prompt, the
   source asset, the camera orbit, and who sent it.
2. Writes the drawing into the session's directory at `feedback/<feedbackId>/annotated.png` and starts the turn
   with a prompt that names that path, so the agent opens exactly what was drawn. The row then names the turn it
   started; a turn that cannot be queued takes the row and its images with it.

The drawing is at most 3.75 MiB, the limit the model APIs set for an image, so the frontend downscales a large capture
before it draws. The clean capture is only kept, never sent, and may be 12 MiB. The answer is
`{ session, turn, feedback }`.

## Deleting

One Delete action opens a confirmation with an unchecked box: "Also permanently delete its files, conversation, and
agent memory".

- Unchecked is a soft delete. The item leaves the grid and appears under the Deleted filter, where it can be restored.
  Its turns stop at once; its session's directory and everything Elysium keeps stay, so a restored item carries on.
- Checked deletes permanently: every file under `studio/<itemId>/` in the storage location first, then the item, its
  assets, feedback, and sessions with their events and directories. When the provider refuses a file, the item stays
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
| `studioItem.upserted`   | `StudioItem`    | An item was created, renamed, pinned, or restored       |
| `studioItem.deleted`    | `{ id }`        | An item was deleted, softly or for good                 |
| `studioAsset.created`   | `StudioAsset`   | A file was pulled from a workspace and stored           |
| `studioFeedback.created`| `StudioFeedback`| A drawn prompt was sent                                 |

Studio sessions publish `session.upserted` and `session.event` like any session.

## Frontend

`frontend/src/pages/Studio/` holds the grid and the item page.

### Grid

`/studio` (`StudioPage`) is tiles only (`StudioItemTile`):

- **Tile.** The thumbnail, title, what the agent is doing, and image and model counts. The state chip shows the
  latest session's activity (`getTileActivityState`); an item with no session has none. The thumbnail is the API's
  choice (`thumbnailAssetId`); without one, a working placeholder shows while a turn runs or waits, else an empty
  frame (`getTileThumbnail` in `studioListing.ts`). A thumbnail whose
  content fails to load (the provider is down, or the file is gone) shows the empty frame too.
- **Filters.** A project (or no project) and a Deleted items switch, kept in the address (`?project=`,
  `?deleted=true`), so a filtered grid survives opening an item. Deleted tiles offer Restore.
- **New item** (`CreateStudioItemForm`). The prompt, an optional title and project, and a storage location. The
  location list follows the project as the API does, and starts on Studio's default when it is among
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
  - Every stored image, on the stage, in the filmstrip, and on the tile, is a `StudioImage`, which shows a placeholder
    in place of an image whose content fails to load rather than an empty box.
  - Annotate is disabled, saying why on hover, while the file shown failed to load: an image that errored, or a model
    the viewer could not show. There is nothing to draw over.
  - The filmstrip (`StudioFilmstrip`) lists every model and image the item holds, then the versions of the one shown,
    newest first, to compare turns. The stage follows the newest version until an older one is picked.
  - Downloads list the newest version of every file, grouped by stem (`groupStudioAssets` in `studioAssets.ts`).
  - The stage opens on a model the viewer can show, else the image the tile shows, else the newest image, else a
    model without a `.glb`, which shows its downloads.
- **Conversation** (`StudioConversation`). One scrolling column of the item's sessions, oldest first, each a
  `StudioSessionTimeline` built on the Coding page's `TimelineEventList`. A divider sits between sessions. A drawn
  prompt shows its drawing under the prompt, matched by `turnId`.
- **Composer** (`StudioComposer`). It wraps the Coding page's `PromptComposer` and sends through `sendStudioTurn`.
  It waits while the item's sessions load, and asks for what to make when the item has never run.
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
  - Elysium's export (`workspace/blender/export_glb.py`) never compresses, so its models need no decoder.
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
