// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// Redux
import { selectIsAdmin } from '../../../store/authSlice'
import { selectBackupsStatus } from '../../../store/backupsSlice'
import { useAppSelector } from '../../../store/hooks'
import { selectAllStorageLocations } from '../../../store/storageLocationsSlice'

// User interface
import { Spinner } from '@heroui/react'
import { EmptyNotice } from '../../../components/EmptyNotice'
import { SettingsPage } from '../SettingsPage'
import { BackupDestinationForm } from './BackupDestinationForm'

// Misc
import { useBackupsLoader, useStorageLocationsLoader } from '../../../hooks/useServerData'
import { UrlTree } from '../../../urls'

// `/settings/backups/destination`: the storage location the databases are backed up to, on a
// page of its own beside the Backups history. Admins only.
export function BackupDestinationPage() {
  const { t } = useTranslation('backups')
  const isAdmin = useAppSelector(selectIsAdmin)

  return <SettingsPage
    title={t('destination.title')}
    parents={[{ label: t('title'), href: UrlTree.settingsBackups }]}
  >
    {isAdmin
      ? <DestinationForm />
      : <EmptyNotice>{t('adminOnly')}</EmptyNotice>}
  </SettingsPage>
}

// Loads the backups' status and the storage locations only for an admin, whom the API answers.
function DestinationForm() {
  const { t } = useTranslation('backups')
  const navigate = useNavigate()
  const loadStatus = useBackupsLoader()
  const loadLocations = useStorageLocationsLoader()
  const status = useAppSelector(selectBackupsStatus)
  const locations = useAppSelector(selectAllStorageLocations)

  if (status && loadLocations !== 'loading') {
    return <BackupDestinationForm
      destinationId={status.destination?.id ?? null}
      locations={locations}
      onSaved={() => void navigate(UrlTree.settingsBackups)}
    />
  }
  if (loadStatus === 'failed' || loadLocations === 'failed') {
    return <p className='py-10 text-center text-sm text-danger'>{t('loadError')}</p>
  }
  return <div className='grid place-items-center py-16'>
    <Spinner />
  </div>
}
