// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { SidebarLink } from './SidebarLink'

// Misc
import { primaryNavigation } from './navigation'

type Props = {
  // Called after a link is followed, so the phone drawer can close itself.
  onNavigate?: () => void
}

// The brand and the primary links: the sidebar from md up, the menu drawer below it.
export function SidebarNavigation(props: Props) {
  const { t } = useTranslation([ 'navigation', 'common' ])

  return <>
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
            onNavigate={props.onNavigate}
          />
        </li>)
      }</ul>
    </nav>
  </>
}
