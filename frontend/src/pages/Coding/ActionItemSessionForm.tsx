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
import { useActionItemLoader, useProjectsLoader } from '../../hooks/useServerData'

type Props = {
  actionItemId: string
  onCreated: () => void
}

// The New session form for a session started from an action item, once the item and the
// projects are loaded. The form presets its project and title from them when it mounts, and
// an item in one project fixes the project picker, so a form mounted before the projects
// arrive could never be submitted. A deleted item
// starts nothing, as the API refuses it.
export function ActionItemSessionForm(props: Props) {
  const { t } = useTranslation([ 'coding', 'common' ])
  const status = useActionItemLoader(props.actionItemId)
  const projectsStatus = useProjectsLoader()
  const actionItem = useAppSelector((state) => selectActionItemById(state, props.actionItemId))

  if (!actionItem && status === 'loading') {
    return <LoadingBody />
  }
  if (!actionItem || actionItem.deletedAt) {
    return <MessageBody message={
      actionItem
        ? t('create.fromItem.deleted')
        : t('create.fromItem.missing')
    } />
  }
  // A form mounted without them could never be submitted, so a failed load says so instead.
  if (projectsStatus === 'failed') {
    return <MessageBody message={t('create.fromItem.presetFailed')} />
  }
  if (projectsStatus === 'loading') {
    return <LoadingBody />
  }

  return <CreateSessionForm
    actionItem={actionItem}
    onCreated={props.onCreated}
  />
}

type MessageBodyProps = {
  message: string
}

// Why no form is shown, with only Close.
function MessageBody(props: MessageBodyProps) {
  const { t } = useTranslation('common')

  return <>
    <Modal.Body>
      <p className='py-6 text-center text-sm opacity-70'>{props.message}</p>
    </Modal.Body>
    <Modal.Footer>
      <Button slot='close' variant='tertiary'>
        <span>{t('actions.close')}</span>
      </Button>
    </Modal.Footer>
  </>
}

function LoadingBody() {
  return <Modal.Body className='grid place-items-center py-10'>
    <Spinner />
  </Modal.Body>
}
