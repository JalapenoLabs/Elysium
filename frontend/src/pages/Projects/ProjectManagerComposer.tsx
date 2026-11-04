// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { projectManagerLoaded } from '../../store/projectManagersSlice'

// User interface
import { Button, Description, Form, Label, TextArea, TextField } from '@heroui/react'
import { LuSendHorizontal } from 'react-icons/lu'

// Misc
import { sendProjectManagerMessage } from '../../api/routes/projectManagerRoutes'
import { submitOnEnter } from '../../components/submitOnEnter'
import { useTextDraft } from '../../drafts/useTextDraft'
import { toast } from '../../toast/toast'

// A message's length, as the API bounds it.
const MESSAGE_MAX_CHARACTERS = 20_000

type Props = {
  projectId: string
}

// Writes to the project's Project Manager, which wakes at once and answers on its turn. What is
// typed is kept for the tab until it is sent.
export function ProjectManagerComposer(props: Props) {
  const { t } = useTranslation('projects')
  const dispatch = useAppDispatch()
  const messageDraft = useTextDraft(`project-manager-message-${props.projectId}`)
  const draft = messageDraft.text ?? ''
  const [ isSending, setIsSending ] = useState(false)

  async function send() {
    const body = draft.trim()
    if (!body || isSending) {
      console.debug('ProjectManagerComposer ignored a send', { isEmpty: !body, isSending })
      return
    }
    setIsSending(true)
    try {
      const response = await sendProjectManagerMessage(props.projectId, body)
      dispatch(projectManagerLoaded(response.projectManager))
      messageDraft.forget()
    }
    catch (error) {
      console.debug('ProjectManagerComposer failed to send a message', { error })
      toast.danger(t('manager.composer.failed'))
    }
    finally {
      setIsSending(false)
    }
  }

  // Enter adds a line; Ctrl or Cmd with Enter sends (ux.md, Hotkeys).
  return <Form
    className='relaxed'
    validationBehavior='aria'
    onKeyDownCapture={submitOnEnter}
    onSubmit={(event) => {
      event.preventDefault()
      void send()
    }}
  >
    <TextField
      id='project-manager-message'
      className='w-full'
      value={draft}
      maxLength={MESSAGE_MAX_CHARACTERS}
      onChange={messageDraft.change}
    >
      <Label className='sr-only'>{t('manager.composer.label')}</Label>
      <TextArea
        rows={2}
        placeholder={t('manager.composer.label')}
      />
      <Description className='text-xs'>{t('manager.composer.hint')}</Description>
    </TextField>
    <div className='mt-2 flex justify-end'>
      <Button type='submit' size='sm' isDisabled={!draft.trim()} isPending={isSending}>
        <LuSendHorizontal className='size-4' aria-hidden />
        <span>{t('manager.composer.send')}</span>
      </Button>
    </div>
  </Form>
}
