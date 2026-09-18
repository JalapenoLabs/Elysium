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
import { useActionItemLoader, useProjectsLoader, useSatellitesLoader } from '../../hooks/useServerData'

type Props = {
  actionItemId: string
  onCreated: () => void
}

// The New session form for a session started from an action item, once the item, the
// projects, and the satellites are loaded. The form presets its project, title, and
// satellite from them when it mounts, and an item in one project fixes the project picker,
// so a form mounted before the projects arrive could never be submitted. A deleted item
// starts nothing, as the API refuses it.
export function ActionItemSessionForm(props: Props) {
  const { t } = useTranslation([ 'coding', 'common' ])
  const status = useActionItemLoader(props.actionItemId)
  const projectsStatus = useProjectsLoader()
  const satellitesStatus = useSatellitesLoader()
  const actionItem = useAppSelector((state) => selectActionItemById(state, props.actionItemId))
  const isPresetLoading = projectsStatus === 'loading' || satellitesStatus === 'loading'

  if (!actionItem && status === 'loading') {
    return <LoadingBody />
  }
  if (!actionItem || actionItem.deletedAt) {
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
  if (isPresetLoading) {
    return <LoadingBody />
  }

  return <CreateSessionForm
    actionItem={actionItem}
    onCreated={props.onCreated}
  />
}

function LoadingBody() {
  return <Modal.Body className='grid place-items-center py-10'>
    <Spinner />
  </Modal.Body>
}
