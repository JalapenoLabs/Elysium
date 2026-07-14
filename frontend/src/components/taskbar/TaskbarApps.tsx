// Copyright © 2026 Jalapeno Labs

import type { IconType } from 'react-icons'

// Core
import { NavLink } from 'react-router'
import { useTranslation } from 'react-i18next'

// UI
import { Tooltip } from '@heroui/react'
import { LuHouse, LuInfo, LuSettings } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../urls'

type TaskbarApp = {
  to: string
  labelKey: string
  Icon: IconType
}

// The centered "apps". Until the rc-dock workspace lands, the routed areas
// stand in as the running apps; once dock panels exist, each open panel renders
// here exactly like a Windows taskbar entry.
const apps: TaskbarApp[] = [
  { to: UrlTree.home, labelKey: 'nav.home', Icon: LuHouse },
  { to: UrlTree.about, labelKey: 'nav.about', Icon: LuInfo },
  { to: UrlTree.settings, labelKey: 'settings.title', Icon: LuSettings },
]

export function TaskbarApps() {
  const { t } = useTranslation('common')

  return <div className='taskbar__apps'>
    { apps.map((app) =>
      <Tooltip key={app.to} content={t(app.labelKey)} placement='top'>
        <NavLink
          to={app.to}
          aria-label={t(app.labelKey)}
          className={({ isActive }) =>
            `taskbar__app ${isActive ? 'taskbar__app--active' : ''}`
          }
        >
          <app.Icon className='h-5 w-5' />
        </NavLink>
      </Tooltip>,
    ) }
  </div>
}
