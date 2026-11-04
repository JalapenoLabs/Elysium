// Copyright © 2026 Jalapeno Labs

import type { WorkspaceSettings } from '../../../api/routes/userRoutes'

// Core
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { workspaceSettingsUpdated } from '../../../store/workspaceSettingsSlice'

// User interface
import { Description, Form, Input, Label, TextField } from '@heroui/react'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { updateWorkspaceSettings } from '../../../api/routes/userRoutes'
import { usePageSave } from '../../../layout/page/pageSave'
import { toast } from '../../../toast/toast'

type Props = {
  settings: WorkspaceSettings
  // Once saved, as an edit page goes back to its list.
  onSaved: () => void
}

// The git repository every agent's Claude Code configuration (CLAUDE.md, skills, settings) is
// checked out from before its turns. The API validates the URL, so its refusal is shown as is.
// Saved from the page's Save, in the header. The field is focused as the page opens, since it is
// the page's one field.
export function AgentConfigurationSettings(props: Props) {
  const { t } = useTranslation([ 'llms', 'common' ])
  const dispatch = useAppDispatch()
  const saved = props.settings.agentConfigurationRepository ?? ''
  const [ repository, setRepository ] = useState(saved)
  const repositoryRef = useRef<HTMLInputElement>(null)
  const trimmed = repository.trim()

  async function save() {
    try {
      // Blank clears the repository, and agents keep the image's default configuration.
      const response = await updateWorkspaceSettings({ agentConfigurationRepository: trimmed || null })
      dispatch(workspaceSettingsUpdated(response.settings))
      toast.success(t('agentConfiguration.saved'))
      props.onSaved()
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

  const onSubmit = usePageSave({
    save,
    isDirty: trimmed !== saved,
  })

  useEffect(() => {
    repositoryRef.current?.focus()
  }, [])

  return <Form className='compact max-w-2xl' onSubmit={onSubmit}>
    <TextField
      id='agent-configuration-repository'
      value={repository}
      autoComplete='url'
      onChange={setRepository}
    >
      <Label>{t('agentConfiguration.repository')}</Label>
      <Input ref={repositoryRef} placeholder='https://github.com/acme/agent-configuration.git' />
      <Description>{t('agentConfiguration.repositoryHint')}</Description>
    </TextField>
  </Form>
}
