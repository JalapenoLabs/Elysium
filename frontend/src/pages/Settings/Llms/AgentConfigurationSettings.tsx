// Copyright © 2026 Jalapeno Labs

import type { WorkspaceSettings } from '../../../api/routes/userRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import { selectAllGithubCredentials } from '../../../store/githubCredentialsSlice'
import { workspaceSettingsUpdated } from '../../../store/workspaceSettingsSlice'

// User interface
import { Description, Form, Input, Label, TextField } from '@heroui/react'
import { GithubTokenSelect, NO_GITHUB_TOKEN } from '../../../components/GithubTokenSelect'
import { AgentConfigurationStatus } from './AgentConfigurationStatus'

// Misc
import { useInitialFocus } from '../../../hooks/useInitialFocus'
import { useGithubCredentialsLoader } from '../../../hooks/useServerData'
import { getApiErrorMessage } from '../../../api/errors'
import { updateWorkspaceSettings } from '../../../api/routes/userRoutes'
import { usePageSave } from '../../../layout/page/pageSave'
import { toast } from '../../../toast/toast'

type Props = {
  settings: WorkspaceSettings
}

// The git repository every worker's and Project Manager's Claude Code configuration (CLAUDE.md,
// skills, docs, settings) is checked out from, and the GitHub token that reads it, which is its
// own: the repository may be private to an account the workspace's default token cannot reach.
// Saving makes the engine fetch it, and how that went shows below as it arrives, so the page
// stays open after a save. The API validates the URL, so its refusal is shown as is.
export function AgentConfigurationSettings(props: Props) {
  const { t } = useTranslation([ 'llms', 'common' ])
  const dispatch = useAppDispatch()
  useGithubCredentialsLoader()
  const credentials = useAppSelector(selectAllGithubCredentials)
  const savedRepository = props.settings.agentConfigurationRepository ?? ''
  const savedToken = props.settings.agentConfigurationGithubCredentialId ?? NO_GITHUB_TOKEN
  const [ repository, setRepository ] = useState(savedRepository)
  const [ token, setToken ] = useState(savedToken)
  const repositoryRef = useInitialFocus()
  const trimmed = repository.trim()

  async function save() {
    try {
      // Blank clears the repository, and workers and Project Managers wait until one is set.
      const response = await updateWorkspaceSettings({
        agentConfigurationRepository: trimmed || null,
        agentConfigurationGithubCredentialId: token === NO_GITHUB_TOKEN
          ? null
          : token,
      })
      dispatch(workspaceSettingsUpdated(response.settings))
      toast.success(t('agentConfiguration.saved'))
      return true
    }
    catch (error) {
      const message = getApiErrorMessage(error)
      if (!message) {
        console.debug('AgentConfigurationSettings could not save the repository', { error })
      }
      toast.danger(message ?? t('common:errors.unexpected'))
      return false
    }
  }

  const isDirty = trimmed !== savedRepository || token !== savedToken
  const onSubmit = usePageSave({
    save,
    isDirty,
  })

  return <>
    <Form className='relaxed max-w-2xl' onSubmit={onSubmit}>
      <TextField
        id='agent-configuration-repository'
        className='compact'
        value={repository}
        autoComplete='url'
        onChange={setRepository}
      >
        <Label>{t('agentConfiguration.repository')}</Label>
        <Input ref={repositoryRef} placeholder='https://github.com/acme/agent-configuration.git' />
        <Description>{t('agentConfiguration.repositoryHint')}</Description>
      </TextField>
      <GithubTokenSelect
        id='agent-configuration-token'
        label={t('agentConfiguration.token')}
        description={t('agentConfiguration.tokenHint')}
        value={token}
        credentials={credentials}
        onChange={setToken}
      />
    </Form>
    <AgentConfigurationStatus
      settings={props.settings}
      hasUnsavedChanges={isDirty}
    />
  </>
}
