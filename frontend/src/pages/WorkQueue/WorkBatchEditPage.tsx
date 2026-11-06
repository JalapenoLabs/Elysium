// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { Navigate, useParams } from 'react-router'

// Redux
import { selectIsCompanyMember } from '../../store/authSlice'
import { useAppSelector } from '../../store/hooks'
import { selectWorkBatch } from '../../store/workQueueSlice'

// User interface
import { Spinner } from '@heroui/react'
import { Page } from '../../layout/page/Page'
import { WorkBatchEditForm } from './WorkBatchEditForm'

// Misc
import { useWorkBatchLoader } from '../../hooks/useServerData'
import { getWorkBatchViewUrl, UrlTree } from '../../urls'

// `/work-queue/:batchId/edit`: a proposed batch's fields, changed and approved in one Save. Only a
// proposal is edited, so any other batch goes back to its own page; a guest is sent to the start.
export function WorkBatchEditPage() {
  const isCompanyMember = useAppSelector(selectIsCompanyMember)
  const { batchId = '' } = useParams()

  if (!isCompanyMember) {
    return <Navigate to={UrlTree.root} replace />
  }
  return <WorkBatchEditView batchId={batchId} />
}

type ViewProps = {
  batchId: string
}

function WorkBatchEditView(props: ViewProps) {
  const { t } = useTranslation('workQueue')
  const status = useWorkBatchLoader(props.batchId, true)
  const loaded = useAppSelector((state) => selectWorkBatch(state, props.batchId))
  const batchUrl = getWorkBatchViewUrl(props.batchId)

  if (loaded && loaded.batch.state !== 'proposed') {
    return <Navigate to={batchUrl} replace />
  }

  return <Page
    title={t('edit.pageTitle')}
    parents={[
      { label: t('title'), href: UrlTree.workQueue },
      { label: loaded?.batch.title ?? t('edit.batch'), href: batchUrl },
    ]}
  >{
      loaded
        ? <WorkBatchEditForm batch={loaded.batch} />
        : status === 'failed'
          ? <p className='py-10 text-center text-sm text-danger'>{t('batch.loadError')}</p>
          : <div className='grid place-items-center py-16'>
            <Spinner />
          </div>
    }</Page>
}
