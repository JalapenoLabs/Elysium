// Copyright © 2026 Jalapeno Labs

import type { PageAction } from './pageActions'

// User interface
import { Dropdown, Label } from '@heroui/react'

// One page action as a menu item: one that acts, a link, or a submenu of its own choices. A
// render function rather than a component, because React Aria builds a menu's collection
// from its direct children.
export function renderPageActionMenuItem(action: PageAction) {
  const Icon = action.icon
  const content = <>
    <Icon className='size-4' aria-hidden />
    <Label>{action.label}</Label>
  </>

  if ('items' in action) {
    return <Dropdown.SubmenuTrigger key={action.id}>
      <Dropdown.Item id={action.id} textValue={action.label} isDisabled={action.isDisabled}>
        {content}
        <Dropdown.SubmenuIndicator />
      </Dropdown.Item>
      <Dropdown.Popover>
        <Dropdown.Menu aria-label={action.label}>{
          action.items.map(renderPageActionMenuItem)
        }</Dropdown.Menu>
      </Dropdown.Popover>
    </Dropdown.SubmenuTrigger>
  }

  return <Dropdown.Item
    key={action.id}
    id={action.id}
    textValue={action.label}
    isDisabled={action.isDisabled}
    variant={action.isDanger
      ? 'danger'
      : 'default'}
    href={'href' in action
      ? action.href
      : undefined}
    onAction={'onAction' in action
      ? action.onAction
      : undefined}
  >{
      content
    }</Dropdown.Item>
}
