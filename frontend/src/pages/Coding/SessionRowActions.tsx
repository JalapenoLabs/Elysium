// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { CodingSession } from '../../api/routes/codingSessionRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Dropdown, Label, Separator, Tooltip } from '@heroui/react'
import { LuEllipsis, LuMessagesSquare, LuPencil, LuTrash2 } from 'react-icons/lu'

// Misc
import { useCodingActions } from './codingActionsContext'

type Props = {
  session: CodingSession
}

// A session tile's "..." menu: open, rename, or delete a session, with the table's row actions'
// icons.
export function SessionRowActions(props: Props) {
  const { t } = useTranslation([ 'coding', 'common' ])
  const codingActions = useCodingActions()

  const actions: Record<string, () => void> = {
    open: () => codingActions.openSession(props.session),
    rename: () => codingActions.renameSession(props.session),
    delete: () => codingActions.deleteSession(props.session),
  }

  return <Dropdown>
    <Tooltip delay={300}>
      <Button
        isIconOnly
        size='sm'
        variant='ghost'
        aria-label={t('common:actions.moreActions')}
      >
        <LuEllipsis className='size-4' aria-hidden />
      </Button>
      <Tooltip.Content>
        <span>{t('common:actions.moreActions')}</span>
      </Tooltip.Content>
    </Tooltip>
    <Dropdown.Popover placement='bottom end'>
      <Dropdown.Menu
        aria-label={t('common:actions.moreActions')}
        onAction={(key: Key) => actions[String(key)]?.()}
      >
        <Dropdown.Item id='open' textValue={t('sessions.open')}>
          <LuMessagesSquare className='size-4' aria-hidden />
          <Label>{t('sessions.open')}</Label>
        </Dropdown.Item>
        <Dropdown.Item id='rename' textValue={t('common:actions.rename')}>
          <LuPencil className='size-4' aria-hidden />
          <Label>{t('common:actions.rename')}</Label>
        </Dropdown.Item>
        <Separator />
        <Dropdown.Item id='delete' textValue={t('common:actions.delete')} variant='danger'>
          <LuTrash2 className='size-4' aria-hidden />
          <Label>{t('common:actions.delete')}</Label>
        </Dropdown.Item>
      </Dropdown.Menu>
    </Dropdown.Popover>
  </Dropdown>
}
