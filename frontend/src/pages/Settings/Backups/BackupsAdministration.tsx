// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { selectBackupsStatus } from '../../../store/backupsSlice'
import { useAppSelector } from '../../../store/hooks'
import { selectAllStorageLocations } from '../../../store/storageLocationsSlice'

// User interface
import { Spinner } from '@heroui/react'
import { LuHardDriveUpload } from 'react-icons/lu'
import { SettingsPage } from '../SettingsPage'
import { BackUpNowButton } from './BackUpNowButton'
import { BackupHistoryTable } from './BackupHistoryTable'
import { BackupsOverview } from './BackupsOverview'

// Misc
import { useBackupsLoader, useStorageLocationsLoader } from '../../../hooks/useServerData'
import { UrlTree } from '../../../urls'

// Loads the backups' status and the storage locations once; the event stream keeps both
// current from then on. Where backups go is chosen on its own page, Destination, from the
// page's actions.
export function BackupsAdministration() {
  const { t } = useTranslation('backups')
  const loadStatus = useBackupsLoader()
  const loadLocations = useStorageLocationsLoader()
  const status = useAppSelector(selectBackupsStatus)
  const locations = useAppSelector(selectAllStorageLocations)

  if (!status || loadLocations === 'loading') {
    const failed = loadStatus === 'failed' || loadLocations === 'failed'
    return <SettingsPage title={t('title')}>{
      failed
        ? <p className='py-10 text-center text-sm text-danger'>{t('loadError')}</p>
        : <div className='grid place-items-center py-16'>
          <Spinner />
        </div>
    }</SettingsPage>
  }

  return <SettingsPage
    title={t('title')}
    primaryAction={<BackUpNowButton status={status} />}
    actions={[{
      id: 'destination',
      label: t('destination.change'),
      icon: LuHardDriveUpload,
      href: UrlTree.settingsBackupsDestination,
    }]}
  >
    <BackupsOverview status={status} />
    <section>
      <h2 className='compact text-xl font-semibold'>{t('history.heading')}</h2>
      <BackupHistoryTable
        backups={status.history}
        locations={locations}
      />
    </section>
  </SettingsPage>
}
