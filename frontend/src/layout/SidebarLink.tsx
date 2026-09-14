// Copyright © 2026 Jalapeno Labs

import type { NavigationItem } from './navigation'

// Core
import { useTranslation } from 'react-i18next'
import { NavLink } from 'react-router'

type Props = {
  item: NavigationItem
}

export function SidebarLink(props: Props) {
  const { t } = useTranslation('navigation')
  const Icon = props.item.icon

  return <NavLink
    to={props.item.href}
    end={props.item.end}
    className={({ isActive }) => [
      'flex items-center gap-2.5 rounded-md px-2 py-1.5 text-sm transition-colors',
      isActive
        ? 'bg-accent/10 font-medium text-accent'
        : 'hover:bg-default',
    ].join(' ')}
  >
    <Icon className='size-4 shrink-0' aria-hidden />
    <span>{
      t(props.item.labelKey)
    }</span>
  </NavLink>
}
