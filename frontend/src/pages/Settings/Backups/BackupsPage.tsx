// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { selectIsAdmin } from '../../../store/authSlice'
import { useAppSelector } from '../../../store/hooks'

// User interface
import { SettingsPage } from '../SettingsPage'
import { BackupsAdministration } from './BackupsAdministration'

// `/settings/backups`: where the databases are backed up every night, and every attempt. Admins
// only; anyone else who opens the address is told so, and the settings directory hides it.
export function BackupsPage() {
  const { t } = useTranslation('backups')
  const isAdmin = useAppSelector(selectIsAdmin)

  if (isAdmin) {
    return <BackupsAdministration />
  }

  return <SettingsPage title={t('title')}>
    <p className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'>{
      t('adminOnly')
    }</p>
  </SettingsPage>
}
