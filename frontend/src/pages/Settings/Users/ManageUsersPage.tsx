// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { selectIsAdmin } from '../../../store/authSlice'
import { useAppSelector } from '../../../store/hooks'

// User interface
import { LuShieldCheck } from 'react-icons/lu'
import { EmptyNotice } from '../../../components/EmptyNotice'
import { SettingsPage } from '../SettingsPage'
import { UsersAdministration } from './UsersAdministration'

// Misc
import { UrlTree } from '../../../urls'

// Users: approving sign-ups and managing people's accounts, with who may join on its own page,
// Workspace access. Admins only; anyone else who opens the address is told so, and the settings
// directory hides the entry from them.
export function ManageUsersPage() {
  const { t } = useTranslation('users')
  const isAdmin = useAppSelector(selectIsAdmin)

  if (!isAdmin) {
    return <SettingsPage title={t('title')}>
      <EmptyNotice>{t('adminOnly')}</EmptyNotice>
    </SettingsPage>
  }

  return <SettingsPage
    title={t('title')}
    actions={[{
      id: 'access',
      label: t('access.title'),
      icon: LuShieldCheck,
      href: UrlTree.settingsUsersAccess,
    }]}
  >
    <UsersAdministration />
  </SettingsPage>
}
