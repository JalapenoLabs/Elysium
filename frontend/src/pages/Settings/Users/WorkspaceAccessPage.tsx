// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// Redux
import { selectIsAdmin } from '../../../store/authSlice'
import { useAppSelector } from '../../../store/hooks'
import { selectWorkspaceSettings } from '../../../store/workspaceSettingsSlice'

// User interface
import { Spinner } from '@heroui/react'
import { EmptyNotice } from '../../../components/EmptyNotice'
import { SettingsPage } from '../SettingsPage'
import { WorkspaceAccessSettings } from './WorkspaceAccessSettings'

// Misc
import { useWorkspaceSettingsLoader } from '../../../hooks/useServerData'
import { UrlTree } from '../../../urls'

// `/settings/users/access`: whether anyone can sign up, and whether everyone must use an
// authenticator app, on a page of their own beside the Users list. Admins only, as the
// workspace settings are.
export function WorkspaceAccessPage() {
  const { t } = useTranslation('users')
  const isAdmin = useAppSelector(selectIsAdmin)

  return <SettingsPage
    title={t('access.title')}
    parents={[{ label: t('title'), href: UrlTree.settingsUsers }]}
  >
    {isAdmin
      ? <WorkspaceAccessForm />
      : <EmptyNotice>{t('adminOnly')}</EmptyNotice>}
  </SettingsPage>
}

// Loads the workspace settings only for an admin, whom the API answers.
function WorkspaceAccessForm() {
  const { t } = useTranslation('users')
  const status = useWorkspaceSettingsLoader()
  const settings = useAppSelector(selectWorkspaceSettings)
  const navigate = useNavigate()

  if (settings) {
    return <WorkspaceAccessSettings
      settings={settings}
      onSaved={() => void navigate(UrlTree.settingsUsers)}
    />
  }
  if (status === 'failed') {
    return <p className='py-10 text-center text-sm text-danger'>{t('access.loadError')}</p>
  }
  return <div className='grid place-items-center py-16'>
    <Spinner />
  </div>
}
