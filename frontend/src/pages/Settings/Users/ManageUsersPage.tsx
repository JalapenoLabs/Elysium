// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { selectIsAdmin } from '../../../store/authSlice'
import { useAppSelector } from '../../../store/hooks'

// User interface
import { SettingsPage } from '../SettingsPage'
import { UsersAdministration } from './UsersAdministration'

// Users: approving sign-ups and managing people's accounts. Admins only; anyone else who
// opens the address is told so, and the settings directory hides the entry from them.
export function ManageUsersPage() {
  const { t } = useTranslation('users')
  const isAdmin = useAppSelector(selectIsAdmin)

  return <SettingsPage title={t('title')}>
    {isAdmin
      ? <UsersAdministration />
      : <p className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'>{
        t('adminOnly')
      }</p>}
  </SettingsPage>
}
