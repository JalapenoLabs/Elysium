// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { selectIsAdmin } from '../../../store/authSlice'
import { useAppSelector } from '../../../store/hooks'
import { selectWorkspaceSettings } from '../../../store/workspaceSettingsSlice'
import { selectWorkQueue } from '../../../store/workQueueSlice'

// User interface
import { Spinner } from '@heroui/react'
import { EmptyNotice } from '../../../components/EmptyNotice'
import { SettingsPage } from '../SettingsPage'
import { WorkerPoolForm } from './WorkerPoolForm'

// Misc
import { useWorkQueueLoader, useWorkspaceSettingsLoader } from '../../../hooks/useServerData'

// `/settings/worker-pool`: how many workers run Project Managers' batches at once. Admins only,
// as the workspace settings are.
export function WorkerPoolPage() {
  const { t } = useTranslation('workQueue')
  const isAdmin = useAppSelector(selectIsAdmin)

  return <SettingsPage title={t('poolSettings.title')} help={t('poolSettings.help')}>
    {isAdmin
      ? <WorkerPoolSettings />
      : <EmptyNotice>{t('poolSettings.adminOnly')}</EmptyNotice>}
  </SettingsPage>
}

// Loads the size an admin chose, and the default the engine derives from its cores, which the
// work queue reports.
function WorkerPoolSettings() {
  const { t } = useTranslation('workQueue')
  const settingsStatus = useWorkspaceSettingsLoader()
  const queueStatus = useWorkQueueLoader(true)
  const settings = useAppSelector(selectWorkspaceSettings)
  const queue = useAppSelector(selectWorkQueue)

  if (settings && queue) {
    return <WorkerPoolForm settings={settings} pool={queue.pool} />
  }
  if (settingsStatus === 'failed' || queueStatus === 'failed') {
    return <p className='py-10 text-center text-sm text-danger'>{t('poolSettings.loadError')}</p>
  }
  return <div className='grid place-items-center py-16'>
    <Spinner />
  </div>
}
