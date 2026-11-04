// Copyright © 2026 Jalapeno Labs

import type { PageAction } from './pageActions'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Dropdown, Separator } from '@heroui/react'
import { LuChevronDown } from 'react-icons/lu'
import { renderPageActionMenuItem } from './renderPageActionMenuItem'

type Props = {
  regular: PageAction[]
  dangerous: PageAction[]
  // Something in the menu is working.
  isPending: boolean
}

// A page's Actions menu: the harmless actions, then the destructive ones below a divider.
export function PageActionsMenu(props: Props) {
  const { t } = useTranslation('common')

  return <Dropdown>
    <Button variant='outline' isPending={props.isPending}>
      <span>{t('page.actions')}</span>
      <LuChevronDown className='size-4' aria-hidden />
    </Button>
    <Dropdown.Popover placement='bottom end'>
      <Dropdown.Menu aria-label={t('page.actions')}>
        {props.regular.map(renderPageActionMenuItem)}
        {Boolean(props.regular.length && props.dangerous.length) && <Separator />}
        {props.dangerous.map(renderPageActionMenuItem)}
      </Dropdown.Menu>
    </Dropdown.Popover>
  </Dropdown>
}
