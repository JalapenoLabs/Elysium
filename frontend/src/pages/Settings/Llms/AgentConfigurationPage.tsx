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
import { AgentConfigurationSettings } from './AgentConfigurationSettings'

// Misc
import { useWorkspaceSettingsLoader } from '../../../hooks/useServerData'
import { UrlTree } from '../../../urls'

// `/settings/llms/agent-configuration`: the repository every agent's Claude Code configuration
// is checked out from, on a page of its own beside the LLM list. Admins only, as the workspace
// settings are.
export function AgentConfigurationPage() {
  const { t } = useTranslation('llms')
  const isAdmin = useAppSelector(selectIsAdmin)

  return <SettingsPage
    title={t('agentConfiguration.title')}
    help={t('agentConfiguration.help')}
    parents={[{ label: t('title'), href: UrlTree.settingsLlms }]}
  >
    {isAdmin
      ? <AgentConfigurationForm />
      : <EmptyNotice>{t('agentConfiguration.adminOnly')}</EmptyNotice>}
  </SettingsPage>
}

// Loads the workspace settings only for an admin, whom the API answers.
function AgentConfigurationForm() {
  const { t } = useTranslation('llms')
  const status = useWorkspaceSettingsLoader()
  const settings = useAppSelector(selectWorkspaceSettings)
  const navigate = useNavigate()

  if (settings) {
    return <AgentConfigurationSettings
      settings={settings}
      onSaved={() => void navigate(UrlTree.settingsLlms)}
    />
  }
  if (status === 'failed') {
    return <p className='py-10 text-center text-sm text-danger'>{t('agentConfiguration.loadError')}</p>
  }
  return <div className='grid place-items-center py-16'>
    <Spinner />
  </div>
}
