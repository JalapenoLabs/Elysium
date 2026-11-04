// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Description, Label, TextArea, TextField, Tooltip } from '@heroui/react'
import { LuSendHorizontal } from 'react-icons/lu'

// Misc
import { submitOnEnter } from '../../components/submitOnEnter'
import { useTextDraft } from '../../drafts/useTextDraft'

type Props = {
  // The field's id, one per conversation, which also names the prompt's draft.
  id: string
  // Sends the prompt and reports its own failures. Resolves true when the prompt was sent,
  // which clears the field; false keeps what was typed so it can be sent again.
  onSend: (prompt: string) => Promise<boolean>
  // Shown on its own row above the field, such as a choice the prompt is sent with. It is
  // not beside the send button, so the field and the button keep the row however narrow
  // the column is.
  header?: ReactNode
  placeholder?: string
  // Why the prompt cannot be sent yet, such as a choice still to make. Set, it disables
  // Send and is shown on hovering it.
  sendBlocker?: string
}

// Sends the next prompt. The turn's progress arrives as live events, so a successful
// send only clears the field.
export function PromptComposer(props: Props) {
  const { t } = useTranslation('coding')
  // What is typed is kept for the tab until it is sent, one draft per conversation.
  const promptDraft = useTextDraft(props.id)
  const prompt = promptDraft.text ?? ''
  const [ isSending, setIsSending ] = useState(false)

  const canSend = !props.sendBlocker && !isSending && prompt.trim().length > 0

  async function send() {
    if (!canSend) {
      console.debug('PromptComposer ignored a send', { isEmpty: !prompt.trim(), isSending, blocker: props.sendBlocker })
      return
    }

    setIsSending(true)
    try {
      const isSent = await props.onSend(prompt)
      if (isSent) {
        promptDraft.forget()
      }
    }
    finally {
      setIsSending(false)
    }
  }

  // Enter adds a line; Ctrl or Cmd with Enter sends (ux.md, Hotkeys).
  return <form
    className='shrink-0 border-t border-separator px-4 py-3'
    onKeyDownCapture={submitOnEnter}
    onSubmit={(event) => {
      event.preventDefault()
      void send()
    }}
  >
    {props.header && <div className='mx-auto mb-2 max-w-3xl'>
      {props.header}
    </div>}
    {/* Send wraps under the field when the column is too narrow for both, so neither is
        squeezed nor pushed past the edge. */}
    <div className='mx-auto flex max-w-3xl flex-wrap items-end justify-end gap-2'>
      <TextField
        id={props.id}
        aria-label={t('conversation.composer.label')}
        className='min-w-0 flex-1 basis-48'
        value={prompt}
        onChange={promptDraft.change}
      >
        <Label className='sr-only'>{t('conversation.composer.label')}</Label>
        <TextArea
          rows={2}
          placeholder={props.placeholder ?? t('conversation.composer.placeholder')}
        />
        <Description className='text-xs'>{t('conversation.composer.hint')}</Description>
      </TextField>
      {/* A disabled button fires no hover, so the reason hangs on a wrapper around it. */}
      <Tooltip delay={200} isDisabled={!props.sendBlocker}>
        <Tooltip.Trigger>
          <div className='mb-6 shrink-0'>
            <Button
              type='submit'
              isDisabled={!canSend}
              isPending={isSending}
            >
              <LuSendHorizontal className='size-4' aria-hidden />
              <span>{t('conversation.composer.send')}</span>
            </Button>
          </div>
        </Tooltip.Trigger>
        <Tooltip.Content className='max-w-xs'>
          <span>{props.sendBlocker}</span>
        </Tooltip.Content>
      </Tooltip>
    </div>
  </form>
}
