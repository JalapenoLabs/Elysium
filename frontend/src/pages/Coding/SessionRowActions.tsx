// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { CodingSession } from '../../api/routes/codingSessionRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Dropdown, Label } from '@heroui/react'
import { LuEllipsis } from 'react-icons/lu'

// Misc
import { useCodingActions } from './codingActionsContext'

type Props = {
  session: CodingSession
}

// The per-row "..." menu: open, rename, or delete a session.
export function SessionRowActions(props: Props) {
  const { t } = useTranslation([ 'coding', 'common' ])
  const codingActions = useCodingActions()

  const actions: Record<string, () => void> = {
    open: () => codingActions.openSession(props.session),
    rename: () => codingActions.renameSession(props.session),
    delete: () => codingActions.deleteSession(props.session),
  }

  return <Dropdown>
    <Button
      isIconOnly
      size='sm'
      variant='ghost'
      aria-label={t('common:actions.moreActions')}
    >
      <LuEllipsis className='size-4' aria-hidden />
    </Button>
    <Dropdown.Popover placement='bottom end'>
      <Dropdown.Menu onAction={(key: Key) => actions[String(key)]?.()}>
        <Dropdown.Item id='open' textValue={t('sessions.open')}>
          <Label>{t('sessions.open')}</Label>
        </Dropdown.Item>
        <Dropdown.Item id='rename' textValue={t('common:actions.rename')}>
          <Label>{t('common:actions.rename')}</Label>
        </Dropdown.Item>
        <Dropdown.Item id='delete' textValue={t('common:actions.delete')} variant='danger'>
          <Label>{t('common:actions.delete')}</Label>
        </Dropdown.Item>
      </Dropdown.Menu>
    </Dropdown.Popover>
  </Dropdown>
}
