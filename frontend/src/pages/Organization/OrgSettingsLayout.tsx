// Copyright © 2026 Jalapeno Labs

// Core
import { NavLink, Outlet } from 'react-router'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'

// UI
import { Accordion, AccordionItem } from '@heroui/react'
import { LuIdCard, LuShieldCheck, LuUsersRound } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../urls'

// The organization settings sidebar is grouped: each group is a collapsible
// accordion of related sections. Groups start expanded so everything is one
// glance away, but can be collapsed on smaller screens.
const groups = [
  {
    key: 'general',
    titleKey: 'orgSettings.groups.general',
    Icon: LuIdCard,
    items: [
      { to: UrlTree.organizationGeneral, labelKey: 'orgSettings.general' },
    ],
  },
  {
    key: 'people',
    titleKey: 'orgSettings.groups.people',
    Icon: LuUsersRound,
    items: [
      { to: UrlTree.organizationMembers, labelKey: 'orgSettings.members' },
      { to: UrlTree.organizationTeams, labelKey: 'orgSettings.teams' },
    ],
  },
  {
    key: 'security',
    titleKey: 'orgSettings.groups.security',
    Icon: LuShieldCheck,
    items: [
      { to: UrlTree.organizationSecurity, labelKey: 'orgSettings.security' },
    ],
  },
] as const

// Organization settings hub: a traditional left sidebar of grouped,
// collapsible navigation over the active org's settings sections. Distinct
// from the simple personal-settings hub (see SettingsLayout).
export function OrgSettingsLayout() {
  const { t } = useTranslation('common')

  const orgs = useAppSelector((state) => state.sessionContext.orgs)
  const activeOrgId = useAppSelector((state) => state.sessionContext.activeOrgId)
  const activeOrg = orgs.find((org) => org.id === activeOrgId)

  return <section className='mx-auto max-w-5xl p-6'>
    <div className='relaxed'>
      <p className='text-xs uppercase tracking-[0.2em] opacity-50'>
        { activeOrg?.name ?? t('orgSettings.fallback') }
      </p>
      <h1 className='text-3xl'>
        <strong>{ t('orgSettings.title') }</strong>
      </h1>
    </div>

    <div className='flex items-start gap-8'>
      <nav className='w-60 shrink-0'>
        <Accordion
          selectionMode='multiple'
          defaultExpandedKeys={groups.map((group) => group.key)}
          showDivider={false}
        >
          { groups.map((group) =>
            <AccordionItem
              key={group.key}
              aria-label={t(group.titleKey)}
              title={<span className='text-sm font-medium'>{ t(group.titleKey) }</span>}
              startContent={<group.Icon className='h-4 w-4 opacity-70' />}
            >
              <div className='flex flex-col gap-1'>
                { group.items.map((item) =>
                  <NavLink
                    key={item.to}
                    to={item.to}
                    className={({ isActive }) =>
                      `rounded-medium px-4 py-2 text-sm transition-colors ${
                        isActive
                          ? 'bg-primary/15 font-medium text-primary'
                          : 'opacity-70 hover:bg-default-100 hover:opacity-100'
                      }`
                    }
                  >
                    { t(item.labelKey) }
                  </NavLink>,
                ) }
              </div>
            </AccordionItem>,
          ) }
        </Accordion>
      </nav>

      <div className='min-w-0 flex-1'>
        <Outlet />
      </div>
    </div>
  </section>
}
