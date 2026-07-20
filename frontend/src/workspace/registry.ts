// Copyright © 2026 Jalapeno Labs

import type { ReactElement, ReactNode } from 'react'

// Core
import { useEffect } from 'react'

// Misc
import { createRegistry } from '../lib/registry'

// The workspace panel roster (docs/workspace.md): a pillar registers panel
// definitions here and the shell does the rest. The taskbar shows a launcher
// per definition, the command palette gains an "open panel" entry, and opening
// one creates a dockable rc-dock tab. The shell knows no pillar by name.
type WorkspacePanelBase = {
  // Stable kind id; also the rc-dock tab id (one open instance per kind).
  id: string
  // Already-translated launcher + tab title. Register from inside React (see
  // useWorkspacePanels) so language changes re-register translated labels.
  label: string
  icon?: ReactNode
}

export type WorkspacePanelDefinition =
  // An in-app React panel.
  | (WorkspacePanelBase & { kind: 'component', render: () => ReactElement })
  // An embedded page (managed service UI, linked external view) hosted in the
  // EmbeddedPanel chrome. M3's forward-auth/SSO makes these arrive
  // pre-authenticated; the shell just renders the URL it is given.
  | (WorkspacePanelBase & { kind: 'embedded', url: string })

const registry = createRegistry<WorkspacePanelDefinition>()

export function registerWorkspacePanels(
  groupId: string,
  definitions: WorkspacePanelDefinition[],
): () => void {
  return registry.register(groupId, definitions)
}

export const listWorkspacePanels = registry.list
export const subscribeWorkspacePanels = registry.subscribe

// Registers panel definitions for the lifetime of the calling component.
// `definitions` must be referentially stable (useMemo).
export function useWorkspacePanels(groupId: string, definitions: WorkspacePanelDefinition[]) {
  useEffect(() => {
    return registry.register(groupId, definitions)
  }, [ groupId, definitions ])
}
