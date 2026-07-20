# Workspace & App Shell

The main screen is the **workspace**: a dockable rc-dock canvas of panels with the bottom
taskbar as its launcher (`docs/navigation.md`). Pillars plug in through one registration
seam; the shell knows no pillar by name. The design language comes from `docs/ux.md`.

## The panel seam

A pillar registers panel definitions and the shell does the rest: a taskbar launcher
appears, the command palette gains an "Open panel" entry, and opening creates a dockable
tab. One call, three surfaces.

```ts
// frontend/src/workspace/registry.ts
useWorkspacePanels('drive', definitions) // inside React; definitions useMemo'd

type WorkspacePanelDefinition =
  | { id, label, icon?, kind: 'component', render: () => ReactElement } // in-app panel
  | { id, label, icon?, kind: 'embedded', url: string }                 // hosted page
```

- `id` is the stable kind id and the rc-dock tab id; one open instance per kind
  (Windows-taskbar semantics). Multi-instance panels are a future extension.
- `label` is registered already-translated from inside React, so language changes
  re-register.
- Registration is grouped and reactive (`frontend/src/lib/registry.ts`, the same seam the
  command palette uses); unmounting the registrar retires the group.

Core registrations live in `workspace/useCoreWorkspacePanels.ts` (mounted by `AppLayout`);
`workspace/useWorkspacePaletteEntries.ts` auto-derives the palette entries.

## The controller

`workspace/controller.ts` separates "open this panel" callers from the mounted dock:

- `openWorkspacePanel(kindId)` opens, or refocuses when already open. Callers on another
  route navigate home and call it in one gesture; requests queue while no workspace is
  mounted and flush when it binds.
- The `Workspace` component (`workspace/Workspace.tsx`, rendered by Home) binds the
  rc-dock instance on mount and reports every layout change back
  (`reportOpenWorkspacePanels`), so the dock layout is the single source of truth for
  what is open and launcher indicators stay honest however a tab was moved or closed.

## Dynamic layout

rc-dock provides the behaviors (group config in `Workspace.tsx`): drag to re-split and
re-panel, float to an in-page window, maximize, and **pop out to a native browser
window** (`newWindow`). A locked, always-present main panel guarantees new tabs a landing
zone; the empty workspace shows the standard empty state. Panel contents are `cached` so
switching tabs keeps component state.

## Embedded panels

`workspace/EmbeddedPanel.tsx` hosts a proxied service UI (managed Forgejo, registry,
mail) or an external linked view in an iframe with standard chrome: the status grammar
dot (`active` while loading, `done` on render), the source URL, and an open-in-browser
escape hatch. The rc-dock tab supplies title/close/drag/float/pop-out.

**Auth seam:** authentication is deliberately not this framework's job. M3's forward-auth
and SSO shell deliver pre-authenticated URLs and cookies, so an embedded tab simply
arrives signed in; the shell renders whatever URL the integration registers. Nothing in
the shell needs reworking when M3 lands.

## Multi-window state

- **Popped-out panels** (rc-dock `new-window`) render through a portal into the child
  window: they stay part of the opener's React tree, so state, Redux, and the realtime
  stream all keep working with no synchronization layer.
- **The dock layout is per-window by design.** Each top-level window arranges its own
  panels; layouts are not mirrored between windows.
- **Session state across real windows** (separate Electron renderer processes, each
  running the full app) rides the existing SharedWorker infrastructure
  (`docs/realtime.md`): one EventSource + one `/context` snapshot fan out to every
  window, so slices stay consistent without window-to-window Redux wiring. This covers
  the primary case today; a broker for user-action state (e.g. cross-window drag of a
  tab) is a roadmap item for the Electron shell.

## Roadmap

- The primary Claude session panel lands on the workspace as the flagship panel (the
  icosahedron interface from CLAUDE.md).
- Layout persistence (save/restore per user) via rc-dock's serializable layout plus
  `loadTab`.
- Drive (#25), git, mail, and action-center surfaces register as panels through this
  seam as their milestones land.
- Cross-window tab drag in the Electron shell.
