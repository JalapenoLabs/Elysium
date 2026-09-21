// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { JiraCredential } from '../../../api/routes/jiraRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Dropdown, Label } from '@heroui/react'
import { LuEllipsis } from 'react-icons/lu'

type Props = {
  credential: JiraCredential
  onEdit: (credential: JiraCredential) => void
  onTest: (credential: JiraCredential) => void
  onDoneTransitions: (credential: JiraCredential) => void
  onDelete: (credential: JiraCredential) => void
}

// The per-row "..." menu: edit, test the connection, choose done statuses, or delete.
export function JiraCredentialRowActions(props: Props) {
  const { t } = useTranslation([ 'jira', 'common' ])

  const actions: Record<string, () => void> = {
    edit: () => props.onEdit(props.credential),
    test: () => props.onTest(props.credential),
    doneTransitions: () => props.onDoneTransitions(props.credential),
    delete: () => props.onDelete(props.credential),
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
        <Dropdown.Item id='edit' textValue={t('common:actions.edit')}>
          <Label>{t('common:actions.edit')}</Label>
        </Dropdown.Item>
        <Dropdown.Item id='test' textValue={t('actions.test')}>
          <Label>{t('actions.test')}</Label>
        </Dropdown.Item>
        <Dropdown.Item id='doneTransitions' textValue={t('actions.doneTransitions')}>
          <Label>{t('actions.doneTransitions')}</Label>
        </Dropdown.Item>
        <Dropdown.Item id='delete' textValue={t('common:actions.delete')} variant='danger'>
          <Label>{t('common:actions.delete')}</Label>
        </Dropdown.Item>
      </Dropdown.Menu>
    </Dropdown.Popover>
  </Dropdown>
}
