// Copyright © 2026 Jalapeno Labs

// The workspace controller: the seam between "open this panel" callers (the
// taskbar, the command palette, future realtime nudges) and the mounted
// rc-dock layout. The dock is the source of truth for what is open; the
// Workspace component binds itself here on mount and reports layout changes
// back, so callers never touch rc-dock directly. Open requests made while no
// workspace is mounted (the user is on another route) queue up and flush on
// bind, letting callers navigate home and open in one gesture.
export type WorkspaceDockBinding = {
  openTab: (kindId: string) => void
  activateTab: (kindId: string) => void
}

let binding: WorkspaceDockBinding | null = null
let pendingOpenKindIds: string[] = []

let openKindIds: string[] = []
const listeners = new Set<() => void>()

export function openWorkspacePanel(kindId: string) {
  if (!binding) {
    pendingOpenKindIds.push(kindId)
    return
  }

  if (openKindIds.includes(kindId)) {
    binding.activateTab(kindId)
  }
  else {
    binding.openTab(kindId)
  }
}

export function bindWorkspaceDock(nextBinding: WorkspaceDockBinding): () => void {
  binding = nextBinding

  const queued = pendingOpenKindIds
  pendingOpenKindIds = []
  for (const kindId of queued) {
    openWorkspacePanel(kindId)
  }

  return () => {
    // Only clear if this binding is still the active one, so remount ordering
    // (new bind before old cleanup) cannot detach the live workspace.
    if (binding === nextBinding) {
      binding = null
    }
  }
}

// Called by the Workspace on every rc-dock layout change with the tab ids the
// layout actually contains. Keeps the taskbar's open/active indicators honest
// no matter how a tab was opened, moved, floated, or closed.
export function reportOpenWorkspacePanels(kindIds: string[]) {
  const isUnchanged = kindIds.length === openKindIds.length
    && kindIds.every((kindId, index) => kindId === openKindIds[index])
  if (isUnchanged) {
    return
  }

  openKindIds = kindIds
  for (const listener of listeners) {
    listener()
  }
}

export function listOpenWorkspacePanelIds(): string[] {
  return openKindIds
}

export function subscribeOpenWorkspacePanels(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}
