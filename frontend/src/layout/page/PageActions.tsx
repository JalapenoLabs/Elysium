// Copyright © 2026 Jalapeno Labs

import type { PageAction } from './pageActions'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, buttonVariants, Dropdown, Label, Link, Separator } from '@heroui/react'
import { LuChevronDown } from 'react-icons/lu'

// Misc
import { arrangePageActions, isAnyPageActionPending } from './pageActions'

type Props = {
  actions: PageAction[]
}

// A page's actions at the right end of its titlebar: a lone harmless action as its own
// button, anything more as one Actions menu with destructive items last. See
// `arrangePageActions` for the rule.
export function PageActions(props: Props) {
  const { t } = useTranslation('common')
  const { button, regular, dangerous } = arrangePageActions(props.actions)

  if (button) {
    return <PageActionButton action={button} />
  }
  if (!regular.length && !dangerous.length) {
    return null
  }

  return <Dropdown>
    <Button variant='outline' isPending={isAnyPageActionPending(props.actions)}>
      <span>{t('page.actions')}</span>
      <LuChevronDown className='size-4' aria-hidden />
    </Button>
    <Dropdown.Popover placement='bottom end'>
      <Dropdown.Menu aria-label={t('page.actions')}>
        {regular.map(renderMenuItem)}
        {Boolean(regular.length && dangerous.length) && <Separator />}
        {dangerous.map(renderMenuItem)}
      </Dropdown.Menu>
    </Dropdown.Popover>
  </Dropdown>
}

type ButtonProps = {
  action: PageAction
}

// The lone action, drawn as the menu item it would otherwise be: a link, a button, or a
// button opening its own choices.
function PageActionButton(props: ButtonProps) {
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
        action.items.map(renderMenuItem)
      }</Dropdown.Menu>
    </Dropdown.Popover>
  </Dropdown>
}

// A render function rather than a component, because React Aria builds a menu's collection
// from its direct children.
function renderMenuItem(action: PageAction) {
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
          action.items.map(renderMenuItem)
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
