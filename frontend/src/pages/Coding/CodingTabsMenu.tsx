// Copyright © 2026 Jalapeno Labs

import type { IDockviewHeaderActionsProps } from 'dockview-react'

// Core
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Dropdown, Label, Tooltip } from '@heroui/react'
import { LuChevronDown, LuLayoutPanelLeft, LuMessagesSquare } from 'react-icons/lu'

// Misc
import { SESSIONS_PANEL_ID } from './codingLayout'

// The end of a panel group's tab strip: once its tabs no longer fit, as on a phone, a menu of
// every tab in the group. It stands in for Dockview's own overflow list, which is a bare
// chevron with no name, no tooltip, and no keyboard access; the strip still scrolls as well.
export function CodingTabsMenu(props: IDockviewHeaderActionsProps) {
  const { t } = useTranslation('coding')
  const rootRef = useRef<HTMLDivElement>(null)
  const [ isOverflowing, setIsOverflowing ] = useState(false)

  // Dockview draws the strip; its `dv-*` classes are the same contract `theme/dockview.css`
  // themes. The strip grows with its tabs until the group is full, so its size, and each
  // tab's, says when to measure again, as does a tab opening or closing.
  useEffect(() => {
    const header = rootRef.current?.closest('.dv-tabs-and-actions-container')
    const strip = header?.querySelector<HTMLElement>('.dv-tabs-container')
    if (!strip) {
      console.debug('CodingTabsMenu could not find its group\'s tab strip', { groupId: props.group.id })
      return undefined
    }
    const measure = () => setIsOverflowing(strip.scrollWidth > strip.clientWidth)
    const observer = new ResizeObserver(measure)
    observer.observe(strip)
    for (const tab of strip.children) {
      observer.observe(tab)
    }
    measure()
    return () => observer.disconnect()
  }, [ props.group.id, props.panels ])

  const label = t('panels.all')

  return <div ref={rootRef} className='flex h-full items-center px-1'>
    {isOverflowing && <Dropdown>
      <Tooltip delay={300}>
        <Button isIconOnly size='sm' variant='ghost' className='size-7 min-w-0' aria-label={label}>
          <LuChevronDown className='size-4' aria-hidden />
        </Button>
        <Tooltip.Content>
          <span>{label}</span>
        </Tooltip.Content>
      </Tooltip>
      <Dropdown.Popover placement='bottom end'>
        <Dropdown.Menu aria-label={label}>{
          props.panels.map((panel) => {
            const Icon = panel.id === SESSIONS_PANEL_ID
              ? LuLayoutPanelLeft
              : LuMessagesSquare
            const title = panel.title ?? panel.id
            return <Dropdown.Item
              key={panel.id}
              id={panel.id}
              textValue={title}
              onAction={() => panel.api.setActive()}
            >
              <Icon className='size-4' aria-hidden />
              <Label>{title}</Label>
            </Dropdown.Item>
          })
        }</Dropdown.Menu>
      </Dropdown.Popover>
    </Dropdown>}
  </div>
}
