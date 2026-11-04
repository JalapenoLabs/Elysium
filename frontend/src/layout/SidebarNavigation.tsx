// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { selectIsCompanyMember } from '../store/authSlice'
import { useAppSelector } from '../store/hooks'

// User interface
import { SidebarBrand } from './SidebarBrand'
import { SidebarLink } from './SidebarLink'

// Misc
import { primaryNavigation } from './navigation'

type Props = {
  // Icons alone, from md up. The phone drawer leaves it off.
  isCollapsed?: boolean
  // Called after a link is followed, so the phone drawer can close itself.
  onNavigate?: () => void
}

// The primary links, then the logo at the foot: the sidebar from md up, the menu drawer below
// it. The parent lays them out as a full-height column.
export function SidebarNavigation(props: Props) {
  const {
    isCollapsed = false,
  } = props

  const { t } = useTranslation('navigation')
  const isCompanyMember = useAppSelector(selectIsCompanyMember)
  const links = primaryNavigation.filter((item) => isCompanyMember || !item.membersOnly)

  return <>
    <nav aria-label={t('primary.label')} className='min-h-0 flex-1 overflow-y-auto overflow-x-hidden'>
      <ul className='flex flex-col gap-1'>{
        links.map((item) => <li key={item.href}>
          <SidebarLink
            item={item}
            isCollapsed={isCollapsed}
            onNavigate={props.onNavigate}
          />
        </li>)
      }</ul>
    </nav>
    <SidebarBrand
      isCollapsed={isCollapsed}
      onNavigate={props.onNavigate}
    />
  </>
}
