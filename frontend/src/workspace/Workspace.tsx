// Copyright © 2026 Jalapeno Labs

import type { BoxBase, LayoutBase, LayoutData, TabData } from 'rc-dock'
import type { WorkspacePanelDefinition } from './registry'

// Core
import { useEffect, useRef, useSyncExternalStore } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { DockLayout } from 'rc-dock'
import { FiGrid } from 'react-icons/fi'
import { EmptyState } from '../components/states/EmptyState'
import { EmbeddedPanel } from './EmbeddedPanel'
import 'rc-dock/dist/rc-dock.css'

// Misc
import {
  bindWorkspaceDock,
  listOpenWorkspacePanelIds,
  reportOpenWorkspacePanels,
  subscribeOpenWorkspacePanels,
} from './controller'
import { listWorkspacePanels } from './registry'

// The dockable workspace (docs/workspace.md): an rc-dock layout hosting every
// open panel. Panels arrive through the workspace controller (taskbar,
// command palette); the dock owns the layout and reports what is open back to
// the controller so launcher indicators stay honest. The main panel is locked
// so the layout always has a landing zone for new tabs.
const MAIN_PANEL_ID = 'workspace-main'
const WORKSPACE_GROUP = 'workspace'

// Drag to re-panel, float, maximize, and pop out to a native window are all
// standard chrome, provided by the group config.
const workspaceGroups = {
  [WORKSPACE_GROUP]: {
    floatable: true,
    maximizable: true,
    newWindow: true,
  },
}

const emptyLayout: LayoutData = {
  dockbox: {
    mode: 'horizontal',
    children: [
      {
        id: MAIN_PANEL_ID,
        tabs: [],
        panelLock: {},
      },
    ],
  },
}

// Walks every region of an rc-dock layout (docked, floating, popped-out,
// maximized) and returns the open tab ids, in layout order.
export function collectTabIds(layout: LayoutBase): string[] {
  const ids: string[] = []

  function walkBox(box: BoxBase | undefined) {
    if (!box) {
      return
    }
    for (const child of box.children) {
      if ('tabs' in child) {
        for (const tab of child.tabs) {
          if (tab.id) {
            ids.push(tab.id)
          }
        }
      }
      else {
        walkBox(child)
      }
    }
  }

  walkBox(layout.dockbox)
  walkBox(layout.floatbox)
  walkBox(layout.windowbox)
  walkBox(layout.maxbox)
  return ids
}

function makeTab(definition: WorkspacePanelDefinition): TabData {
  return {
    id: definition.id,
    title: definition.label,
    group: WORKSPACE_GROUP,
    closable: true,
    // Keep panel component state alive while other tabs are focused.
    cached: true,
    content: definition.kind === 'embedded'
      ? <EmbeddedPanel title={definition.label} url={definition.url} />
      : definition.render(),
  }
}

export function Workspace() {
  const { t } = useTranslation()
  const dockRef = useRef<DockLayout>(null)

  const openKindIds = useSyncExternalStore(
    subscribeOpenWorkspacePanels,
    listOpenWorkspacePanelIds,
  )

  useEffect(() => {
    return bindWorkspaceDock({
      openTab: (kindId) => {
        const definition = listWorkspacePanels().find((panel) => panel.id === kindId)
        const dock = dockRef.current
        if (!definition || !dock) {
          console.debug('Workspace cannot open an unregistered or unmounted panel', { kindId })
          return
        }
        dock.dockMove(makeTab(definition), dock.find(MAIN_PANEL_ID) ?? null, 'middle')
      },
      activateTab: (kindId) => {
        dockRef.current?.updateTab(kindId, null, true)
      },
    })
  }, [])

  return <div className='workspace'>
    <DockLayout
      ref={dockRef}
      defaultLayout={emptyLayout}
      groups={workspaceGroups}
      onLayoutChange={(layout) => {
        reportOpenWorkspacePanels(collectTabIds(layout))
      }}
      style={{ position: 'absolute', inset: 0 }}
    />
    {!openKindIds.length && (
      <div className='workspace__empty'>
        <EmptyState
          icon={<FiGrid />}
          title={t('workspace.emptyTitle')}
          description={t('workspace.emptyDescription')}
        />
      </div>
    )}
  </div>
}
