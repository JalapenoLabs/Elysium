// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { selectActionItemById } from '../../store/actionItemsSlice'
import { useAppSelector } from '../../store/hooks'

// User interface
import { Spinner } from '@heroui/react'
import { EmptyNotice } from '../../components/EmptyNotice'
import { CreateSessionForm } from './CreateSessionForm'

// Misc
import { useActionItemLoader, useProjectsLoader } from '../../hooks/useServerData'

type Props = {
  actionItemId: string
}

// The New session form for a session started from an action item, once the item and the
// projects are loaded. The form presets its project and title from them when it mounts, and
// an item in one project fixes the project picker, so a form mounted before the projects
// arrive could never be submitted. A deleted item starts nothing, as the API refuses it.
export function ActionItemSessionForm(props: Props) {
  const { t } = useTranslation('coding')
  const status = useActionItemLoader(props.actionItemId)
  const projectsStatus = useProjectsLoader()
  const actionItem = useAppSelector((state) => selectActionItemById(state, props.actionItemId))

  if (!actionItem && status === 'loading') {
    return <Loading />
  }
  if (!actionItem || actionItem.deletedAt) {
    return <EmptyNotice>{
      actionItem
        ? t('create.fromItem.deleted')
        : t('create.fromItem.missing')
    }</EmptyNotice>
  }
  // A form mounted without them could never be submitted, so a failed load says so instead.
  if (projectsStatus === 'failed') {
    return <EmptyNotice>{t('create.fromItem.presetFailed')}</EmptyNotice>
  }
  if (projectsStatus === 'loading') {
    return <Loading />
  }

  return <CreateSessionForm actionItem={actionItem} />
}

function Loading() {
  return <div className='grid place-items-center py-16'>
    <Spinner />
  </div>
}
