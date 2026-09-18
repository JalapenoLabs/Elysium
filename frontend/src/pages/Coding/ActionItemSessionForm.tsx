// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { selectActionItemById } from '../../store/actionItemsSlice'
import { useAppSelector } from '../../store/hooks'

// User interface
import { Button, Modal, Spinner } from '@heroui/react'
import { CreateSessionForm } from './CreateSessionForm'

// Misc
import { useActionItemLoader } from '../../hooks/useServerData'

type Props = {
  actionItemId: string
  onCreated: () => void
}

// The New session form for a session started from an action item, once the item is loaded.
// The form presets itself from the item when it mounts, so it waits for it here. A deleted
// item starts nothing, as the API refuses it.
export function ActionItemSessionForm(props: Props) {
  const { t } = useTranslation([ 'coding', 'common' ])
  const status = useActionItemLoader(props.actionItemId)
  const actionItem = useAppSelector((state) => selectActionItemById(state, props.actionItemId))

  if (actionItem && !actionItem.deletedAt) {
    return <CreateSessionForm
      actionItem={actionItem}
      onCreated={props.onCreated}
    />
  }

  if (!actionItem && status === 'loading') {
    return <Modal.Body className='grid place-items-center py-10'>
      <Spinner />
    </Modal.Body>
  }

  return <>
    <Modal.Body>
      <p className='py-6 text-center text-sm opacity-70'>{
        actionItem
          ? t('create.fromItem.deleted')
          : t('create.fromItem.missing')
      }</p>
    </Modal.Body>
    <Modal.Footer>
      <Button slot='close' variant='tertiary'>
        <span>{t('common:actions.close')}</span>
      </Button>
    </Modal.Footer>
  </>
}
