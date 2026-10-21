// Copyright © 2026 Jalapeno Labs

import type { FormEvent } from 'react'
import type { WorkspaceSettings } from '../../../api/routes/userRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { workspaceSettingsUpdated } from '../../../store/workspaceSettingsSlice'

// User interface
import { Button, Description, Form, Input, Label, TextField, toast } from '@heroui/react'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { updateWorkspaceSettings } from '../../../api/routes/userRoutes'

type Props = {
  settings: WorkspaceSettings
}

// The git repository every agent's Claude Code configuration (CLAUDE.md, skills, settings) is
// checked out from before its turns. The API validates the URL, so its refusal is shown as is.
export function AgentConfigurationSettings(props: Props) {
  const { t } = useTranslation([ 'llms', 'common' ])
  const dispatch = useAppDispatch()
  const saved = props.settings.agentConfigurationRepository ?? ''
  const [ repository, setRepository ] = useState(saved)
  const [ isSaving, setIsSaving ] = useState(false)
  const trimmed = repository.trim()

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setIsSaving(true)
    try {
      // Blank clears the repository, and agents keep the image's default configuration.
      const response = await updateWorkspaceSettings({ agentConfigurationRepository: trimmed || null })
      dispatch(workspaceSettingsUpdated(response.settings))
      toast.success(t('agentConfiguration.saved'))
    }
    catch (error) {
      const message = getApiErrorMessage(error)
      if (!message) {
        console.debug('AgentConfigurationSettings could not save the repository', { error })
      }
      toast.danger(message ?? t('common:errors.unexpected'))
    }
    setIsSaving(false)
  }

  return <Form className='compact flex flex-wrap items-end gap-3' onSubmit={save}>
    <TextField
      className='min-w-72 flex-1'
      value={repository}
      onChange={setRepository}
    >
      <Label>{t('agentConfiguration.repository')}</Label>
      <Input placeholder='https://github.com/acme/agent-configuration.git' />
      <Description>{t('agentConfiguration.repositoryHint')}</Description>
    </TextField>
    <Button
      type='submit'
      variant='outline'
      isDisabled={trimmed === saved}
      isPending={isSaving}
    >
      <span>{t('common:actions.save')}</span>
    </Button>
  </Form>
}
