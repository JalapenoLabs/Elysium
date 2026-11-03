// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { selectIsAdmin } from '../../../store/authSlice'
import { useAppSelector } from '../../../store/hooks'

// User interface
import { SettingsPage } from '../SettingsPage'
import { UpdatesAdministration } from './UpdatesAdministration'

// Updates: the release running, auto-update's state, and the update history. Admins only;
// anyone else who opens the address is told so, and the settings directory hides the entry.
export function UpdatesPage() {
  const { t } = useTranslation('updates')
  const isAdmin = useAppSelector(selectIsAdmin)

  return <SettingsPage title={t('title')}>
    {isAdmin
      ? <UpdatesAdministration />
      : <p className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'>{
        t('adminOnly')
      }</p>}
  </SettingsPage>
}
