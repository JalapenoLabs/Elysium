// Copyright © 2026 Jalapeno Labs

import type { PageCrumb } from '../../layout/page/PageHeader'

// Core
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectInitiativeById } from '../../store/initiativesSlice'

// User interface
import { Spinner } from '@heroui/react'
import { EmptyNotice } from '../../components/EmptyNotice'
import { Page } from '../../layout/page/Page'
import { AddContainerForm } from './AddContainerForm'

// Misc
import { useInitiativeLoader } from '../../hooks/useServerData'
import { getInitiativeViewUrl, UrlTree } from '../../urls'

// `/action-items/initiatives/:initiativeId/containers/new`: following a Jira epic or saved
// filter, or a GitHub milestone or label, on one initiative. A deleted initiative follows
// nothing new, as its page is read-only until it is restored.
export function NewInitiativeContainerPage() {
  const { t } = useTranslation([ 'initiatives', 'actionItems' ])
  const { initiativeId = '' } = useParams()
  const status = useInitiativeLoader(initiativeId)
  const initiative = useAppSelector((state) => selectInitiativeById(state, initiativeId))

  const parents: PageCrumb[] = [
    { label: t('actionItems:title'), href: UrlTree.actionItems },
    { label: t('title'), href: UrlTree.initiatives },
  ]
  // Back goes to the initiative, once it is known to exist.
  if (initiative) {
    parents.push({ label: initiative.name, href: getInitiativeViewUrl(initiative.id) })
  }

  return <Page
    title={t('containers.newTitle')}
    parents={parents}
    help={t('containers.addDescription')}
  >
    {!initiative && status === 'loading' && <div className='grid place-items-center py-16'>
      <Spinner />
    </div>}
    {!initiative && status !== 'loading' && <EmptyNotice>{t('page.notFound')}</EmptyNotice>}
    {initiative?.deletedAt && <EmptyNotice>{t('containers.deleted')}</EmptyNotice>}
    {initiative && !initiative.deletedAt && <AddContainerForm initiativeId={initiative.id} />}
  </Page>
}
