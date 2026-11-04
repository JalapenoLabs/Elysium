// Copyright © 2026 Jalapeno Labs

import type { Project } from '../../api/routes/projectRoutes'

// Core
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// User interface
import { Spinner } from '@heroui/react'
import { Page } from '../../layout/page/Page'
import { WorkerSetupForm } from './WorkerSetupForm'

// Misc
import { getProjectWorkerSetup, getProjectWorkerSetupKey } from '../../api/routes/workerSetupRoutes'
import { getProjectViewUrl, UrlTree } from '../../urls'

type Props = {
  project: Project
}

// How one project readies its workers' containers: the tools setup script, and the repositories
// it declares, each with an optional base branch and setup script. Saved by the page's Save.
//
// The setup is read through SWR alone, as a project's models are: no other view shows it, and
// `projectWorkerSetup.changed` revalidates its key while the page holds it.
export function ProjectWorkers(props: Props) {
  const { t } = useTranslation('projects')
  const project = props.project
  const listing = useSWR(
    getProjectWorkerSetupKey(project.id),
    () => getProjectWorkerSetup(project.id),
  )

  return <Page
    title={t('workers.title')}
    help={t('workers.help')}
    parents={[
      { label: t('title'), href: UrlTree.projects },
      { label: project.name, href: getProjectViewUrl(project.id) },
    ]}
  >
    {!listing.data && !listing.error && <div className='grid place-items-center py-16'>
      <Spinner />
    </div>}

    {!listing.data && listing.error && <p className='py-10 text-center text-sm text-danger'>{
      t('workers.loadError')
    }</p>}

    {listing.data && <WorkerSetupForm
      projectId={project.id}
      setup={listing.data.workerSetup}
    />}
  </Page>
}
