// Copyright © 2026 Jalapeno Labs

import type { PageAction } from './pageActions'

// User interface
import { Button, buttonVariants, Dropdown, Link } from '@heroui/react'
import { LuChevronDown } from 'react-icons/lu'
import { renderPageActionMenuItem } from './renderPageActionMenuItem'

// Misc
import { isAnyPageActionPending } from './pageActions'

type Props = {
  action: PageAction
}

// A page's lone action, drawn as the menu item it would otherwise be: a link, a button, or a
// button opening its own choices.
export function PageActionButton(props: Props) {
  const action = props.action
  const Icon = action.icon
  const content = <>
    <Icon className='size-4' aria-hidden />
    <span>{action.label}</span>
  </>

  if ('href' in action) {
    return <Link href={action.href} className={buttonVariants({ variant: 'outline' })}>{
      content
    }</Link>
  }
  if ('onAction' in action) {
    return <Button
      variant='outline'
      isDisabled={action.isDisabled}
      isPending={action.isPending}
      onPress={action.onAction}
    >{
        content
      }</Button>
  }
  return <Dropdown>
    <Button variant='outline' isDisabled={action.isDisabled} isPending={isAnyPageActionPending(action.items)}>
      {content}
      <LuChevronDown className='size-4' aria-hidden />
    </Button>
    <Dropdown.Popover placement='bottom end'>
      <Dropdown.Menu aria-label={action.label}>{
        action.items.map(renderPageActionMenuItem)
      }</Dropdown.Menu>
    </Dropdown.Popover>
  </Dropdown>
}
