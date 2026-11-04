// Copyright © 2026 Jalapeno Labs

import type { NavigationItem } from './navigation'

// Core
import { useTranslation } from 'react-i18next'
import { useMatch } from 'react-router'

// User interface
import { Link } from 'react-aria-components'
import { SidebarTooltip } from './SidebarTooltip'

type Props = {
  item: NavigationItem
  isCollapsed: boolean
  onNavigate?: () => void
}

// One primary link. Hovering fills it neutrally and the current page is accent-colored with a
// bar at its edge, so the page you are on and the one under the mouse never look alike
// (`~/.claude/docs/ux.md`, Navigation). Collapsed, the sidebar's width clips the label away
// behind the icon, and the label stays the link's accessible name.
//
// React Aria's unstyled `Link` rather than React Router's `NavLink`: it takes the collapsed
// tooltip's hover and focus handlers, reports hover only for a real mouse, so a tap leaves no
// stuck highlight, and still routes in the app through the `RouterProvider` in `AppShell`.
export function SidebarLink(props: Props) {
  const { t } = useTranslation('navigation')
  const isActive = Boolean(useMatch({ path: props.item.href, end: props.item.end }))
  const Icon = props.item.icon
  const label = t(props.item.labelKey)

  return <SidebarTooltip label={label} isEnabled={props.isCollapsed}>
    <Link
      href={props.item.href}
      aria-current={isActive
        ? 'page'
        : undefined}
      onPress={props.onNavigate}
      className={[
        'relative flex items-center gap-3 overflow-hidden rounded-lg px-3.5 py-2.5 text-base outline-none',
        'transition-colors duration-200 ease-out motion-reduce:transition-none',
        'data-focus-visible:ring-2 data-focus-visible:ring-focus data-focus-visible:ring-inset',
        isActive
          ? 'bg-accent/15 font-medium text-accent'
          : 'text-foreground/75 data-hovered:bg-default data-hovered:text-foreground',
      ].join(' ')}
    >
      <span
        aria-hidden
        className={[
          'absolute inset-y-2 left-0 w-1 rounded-full bg-accent',
          'transition-transform duration-200 ease-out motion-reduce:transition-none',
          isActive
            ? 'scale-y-100'
            : 'scale-y-0',
        ].join(' ')}
      />
      <Icon className='size-5 shrink-0' aria-hidden />
      <span className={[
        'whitespace-nowrap transition-opacity duration-200 ease-out motion-reduce:transition-none',
        props.isCollapsed
          ? 'opacity-0'
          : 'opacity-100',
      ].join(' ')}>{
        label
      }</span>
    </Link>
  </SidebarTooltip>
}
