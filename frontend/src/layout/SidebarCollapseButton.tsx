// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch, useAppSelector } from '../store/hooks'
import { selectIsSidebarCollapsed, sidebarCollapsedChanged } from '../store/sidebarSlice'

// User interface
import { Button, Tooltip } from '@heroui/react'
import { LuChevronLeft } from 'react-icons/lu'

// The small round chevron on the sidebar's edge, floating slightly over the page. It shows
// while the mouse is anywhere over the sidebar (the parent is `group/sidebar`), while it has
// keyboard focus, and always on touch screens, which have no hover to reveal it. It sits level
// with the middle of the topbar, beside the account menu (`~/.claude/docs/ux.md`, Sidebars).
export function SidebarCollapseButton() {
  const { t } = useTranslation('navigation')
  const dispatch = useAppDispatch()
  const isCollapsed = useAppSelector(selectIsSidebarCollapsed)
  const label = isCollapsed
    ? t('sidebar.expand')
    : t('sidebar.collapse')

  return <Tooltip delay={300}>
    <Button
      isIconOnly
      size='sm'
      variant='outline'
      aria-label={label}
      aria-expanded={!isCollapsed}
      onPress={() => dispatch(sidebarCollapsedChanged(!isCollapsed))}
      className={[
        'absolute top-5 -right-3 z-30 size-6 min-w-0 rounded-full bg-surface shadow-sm',
        'opacity-0 transition-opacity duration-200 motion-reduce:transition-none',
        'group-hover/sidebar:opacity-100 focus-visible:opacity-100 pointer-coarse:opacity-100',
      ].join(' ')}
    >
      <LuChevronLeft
        aria-hidden
        className={[
          'size-3.5 transition-transform duration-200 motion-reduce:transition-none',
          isCollapsed
            ? 'rotate-180'
            : 'rotate-0',
        ].join(' ')}
      />
    </Button>
    <Tooltip.Content placement='right'>
      <span>{
        label
      }</span>
    </Tooltip.Content>
  </Tooltip>
}
