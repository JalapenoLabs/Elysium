// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { selectIsAdmin } from '../../../store/authSlice'
import { useAppSelector } from '../../../store/hooks'

// User interface
import { SettingsPage } from '../SettingsPage'
import { PasswordVaultAdministration } from './PasswordVaultAdministration'

// Passwords: the vault agents sign in to things with. Admins only; anyone else who opens the
// address is told so, and the settings directory hides the entry.
export function PasswordsPage() {
  const { t } = useTranslation('passwords')
  const isAdmin = useAppSelector(selectIsAdmin)

  return <SettingsPage title={t('title')}>
    {isAdmin
      ? <PasswordVaultAdministration />
      : <p className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'>{
        t('adminOnly')
      }</p>}
  </SettingsPage>
}
