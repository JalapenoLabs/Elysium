// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// UI
import { Button, Tooltip } from '@heroui/react'
import { LuMoon, LuSun } from 'react-icons/lu'
import { OrgSwitcher } from './OrgSwitcher'
import { ProjectSwitcher } from './ProjectSwitcher'
import { TaskbarSearch } from './TaskbarSearch'
import { TaskbarApps } from './TaskbarApps'
import { ProfileBadge } from '../user/ProfileBadge'
import { StatusControl } from '../user/StatusControl'

// Redux
import { dispatch } from '../../store'
import { useAppSelector } from '../../store/hooks'
import { toggleTheme } from '../../store/themeSlice'

// Elysium's chrome is a bottom taskbar, Windows-11 style; the app deliberately
// has no topbar. Left: who and where you are (org + profile). Center: search
// and the running "apps". Right: the tray (status, theme, clock). The three
// zones are grid columns so the center stays visually centered.
export function Taskbar() {
  const { t } = useTranslation('common')
  const themeMode = useAppSelector((state) => state.theme.mode)

  return <footer className='taskbar'>
    <div className='taskbar__zone taskbar__zone--left'>
      <OrgSwitcher />
      <ProjectSwitcher />
      <ProfileBadge />
    </div>

    <div className='taskbar__zone taskbar__zone--center'>
      <TaskbarSearch />
      <TaskbarApps />
    </div>

    <div className='taskbar__zone taskbar__zone--right'>
      <StatusControl />
      <Tooltip content={t('theme.toggle')} placement='top'>
        <div>
          <Button
            isIconOnly
            variant='light'
            aria-label={t('theme.toggle')}
            onPress={() => dispatch(toggleTheme())}
          >
            { themeMode === 'dark'
              ? <LuMoon className='h-4 w-4' />
              : <LuSun className='h-4 w-4' /> }
          </Button>
        </div>
      </Tooltip>
    </div>
  </footer>
}
