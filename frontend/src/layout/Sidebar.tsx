// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { SidebarLink } from './SidebarLink'

// Misc
import { primaryNavigation } from './navigation'

export function Sidebar() {
  const { t } = useTranslation([ 'navigation', 'common' ])

  return <aside className='flex w-56 shrink-0 flex-col border-r border-separator bg-sidebar px-3 py-4'>
    <div className='level-left relaxed px-2'>
      <div className='grid size-8 place-items-center rounded-lg bg-accent text-sm font-bold text-accent-foreground'>
        E
      </div>
      <span className='text-sm font-semibold'>{
        t('common:brand.name')
      }</span>
    </div>
    <nav aria-label={t('primary.label')}>
      <ul className='flex flex-col gap-0.5'>{
        primaryNavigation.map((item) => <li key={item.href}>
          <SidebarLink
            item={item}
          />
        </li>)
      }</ul>
    </nav>
  </aside>
}
