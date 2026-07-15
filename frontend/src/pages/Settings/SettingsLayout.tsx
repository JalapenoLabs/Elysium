// Copyright © 2026 Jalapeno Labs

// Core
import { NavLink, Outlet } from 'react-router'
import { useTranslation } from 'react-i18next'

// Misc
import { UrlTree } from '../../urls'

// Personal account settings only ("me" concerns). Deliberately simple: a flat
// sidebar, no grouping. Organization-level settings live in their own hub with
// a richer accordion sidebar (see OrgSettingsLayout).
const sections = [
  { to: UrlTree.settingsProfile, labelKey: 'settings.profile' },
  { to: UrlTree.settingsSecurity, labelKey: 'settings.security' },
  { to: UrlTree.settingsSessions, labelKey: 'settings.sessions' },
] as const

export function SettingsLayout() {
  const { t } = useTranslation('common')

  return <section className='mx-auto max-w-4xl p-6'>
    <div className='relaxed'>
      <h1 className='text-3xl'>
        <strong>{ t('settings.title') }</strong>
      </h1>
    </div>

    <div className='flex items-start gap-8'>
      <nav className='flex w-48 shrink-0 flex-col gap-1'>
        { sections.map((section) =>
          <NavLink
            key={section.to}
            to={section.to}
            className={({ isActive }) =>
              `rounded-medium px-4 py-2 text-sm transition-colors ${
                isActive
                  ? 'bg-primary/15 font-medium text-primary'
                  : 'opacity-70 hover:bg-default-100 hover:opacity-100'
              }`
            }
          >
            { t(section.labelKey) }
          </NavLink>,
        ) }
      </nav>

      <div className='min-w-0 flex-1'>
        <Outlet />
      </div>
    </div>
  </section>
}
