// Copyright © 2026 Jalapeno Labs

// Core
import { useSyncExternalStore } from 'react'
import { NavLink, useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// UI
import { Tooltip } from '@heroui/react'
import { LuHouse, LuPanelsTopLeft, LuSettings } from 'react-icons/lu'

// Misc
import {
  listOpenWorkspacePanelIds,
  openWorkspacePanel,
  subscribeOpenWorkspacePanels,
} from '../../workspace/controller'
import { listWorkspacePanels, subscribeWorkspacePanels } from '../../workspace/registry'
import { UrlTree } from '../../urls'

// The centered "apps", Windows-taskbar style: the workspace itself (Home),
// one launcher per registered workspace panel (the running indicator lights
// while its dock tab is open), and Settings. Clicking a launcher lands on the
// workspace and opens or refocuses that panel.
export function TaskbarApps() {
  const { t } = useTranslation('common')
  const navigate = useNavigate()

  const panels = useSyncExternalStore(subscribeWorkspacePanels, listWorkspacePanels)
  const openKindIds = useSyncExternalStore(
    subscribeOpenWorkspacePanels,
    listOpenWorkspacePanelIds,
  )

  return <div className='taskbar__apps'>
    <Tooltip content={t('nav.home')} placement='top'>
      <NavLink
        to={UrlTree.home}
        aria-label={t('nav.home')}
        className={({ isActive }) =>
          `taskbar__app ${isActive ? 'taskbar__app--active' : ''}`
        }
      >
        <LuHouse className='h-5 w-5' />
      </NavLink>
    </Tooltip>
    { panels.map((panel) =>
      <Tooltip key={panel.id} content={panel.label} placement='top'>
        <button
          type='button'
          aria-label={panel.label}
          className={
            `taskbar__app ${openKindIds.includes(panel.id) ? 'taskbar__app--active' : ''}`
          }
          onClick={() => {
            navigate(UrlTree.home)
            openWorkspacePanel(panel.id)
          }}
        >
          { panel.icon ?? <LuPanelsTopLeft className='h-5 w-5' /> }
        </button>
      </Tooltip>,
    ) }
    <Tooltip content={t('settings.title')} placement='top'>
      <NavLink
        to={UrlTree.settings}
        aria-label={t('settings.title')}
        className={({ isActive }) =>
          `taskbar__app ${isActive ? 'taskbar__app--active' : ''}`
        }
      >
        <LuSettings className='h-5 w-5' />
      </NavLink>
    </Tooltip>
  </div>
}
