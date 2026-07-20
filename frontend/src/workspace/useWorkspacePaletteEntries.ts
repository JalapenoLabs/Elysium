// Copyright © 2026 Jalapeno Labs

import type { PaletteEntry } from '../components/palette/registry'

// Core
import { useMemo, useSyncExternalStore } from 'react'
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// Misc
import { usePaletteEntries } from '../components/palette/registry'
import { listWorkspacePanels, subscribeWorkspacePanels } from './registry'
import { openWorkspacePanel } from './controller'
import { UrlTree } from '../urls'

// Every registered workspace panel automatically becomes a command-palette
// entry ("Open <panel>"): registering a panel is one call, and the palette,
// taskbar, and dock all light up together. Opening navigates home first; the
// controller queues the open until the workspace binds.
export function useWorkspacePaletteEntries() {
  const { t } = useTranslation()
  const navigate = useNavigate()

  const panels = useSyncExternalStore(subscribeWorkspacePanels, listWorkspacePanels)

  const entries = useMemo<PaletteEntry[]>(() => panels.map((panel) => ({
    id: `workspace.open.${panel.id}`,
    label: panel.label,
    section: t('palette.sections.openPanel'),
    icon: panel.icon,
    run: () => {
      navigate(UrlTree.home)
      openWorkspacePanel(panel.id)
    },
  })), [ panels, t, navigate ])

  usePaletteEntries('workspace.panels', entries)
}
