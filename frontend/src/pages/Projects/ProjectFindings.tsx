// Copyright © 2026 Jalapeno Labs

import type { Finding } from '../../api/routes/findingRoutes'
import type { LoadStatus } from '../../hooks/useServerData'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Spinner } from '@heroui/react'
import { EmptyNotice } from '../../components/EmptyNotice'
import { ProjectFindingsTable } from './ProjectFindingsTable'

type Props = {
  status: LoadStatus
  findings: Finding[]
}

// A project's Findings tab: work its workers and Project Manager noticed that nobody asked for,
// waiting for a person to accept or dismiss. `ProjectLists` loads the list for the tab's count.
export function ProjectFindings(props: Props) {
  const { t } = useTranslation('projects')

  if (props.status === 'loading') {
    return <div className='grid place-items-center py-10'>
      <Spinner />
    </div>
  }
  if (props.status === 'failed') {
    return <EmptyNotice>{t('findings.loadError')}</EmptyNotice>
  }
  if (!props.findings.length) {
    return <EmptyNotice>{t('findings.empty')}</EmptyNotice>
  }
  return <ProjectFindingsTable findings={props.findings} />
}
