// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Description, Label, TextArea, TextField, Tooltip } from '@heroui/react'
import { LuSendHorizontal } from 'react-icons/lu'

type Props = {
  // Sends the prompt and reports its own failures. Resolves true when the prompt was sent,
  // which clears the field; false keeps what was typed so it can be sent again.
  onSend: (prompt: string) => Promise<boolean>
  // The thread has ended and cannot take prompts.
  isClosed: boolean
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
  const [ prompt, setPrompt ] = useState('')
  const [ isSending, setIsSending ] = useState(false)

  const canSend = !props.isClosed && !props.sendBlocker && !isSending && prompt.trim().length > 0

  async function send() {
    if (!canSend) {
      return
    }

    setIsSending(true)
    try {
      const isSent = await props.onSend(prompt)
      if (isSent) {
        setPrompt('')
      }
    }
    finally {
      setIsSending(false)
    }
  }

  if (props.isClosed) {
    return <p className='shrink-0 border-t border-separator px-4 py-3 text-center text-sm opacity-60'>{
      t('conversation.composer.closed')
    }</p>
  }

  return <form
    className='shrink-0 border-t border-separator px-4 py-3'
    onSubmit={(event) => {
      event.preventDefault()
      send()
    }}
  >
    {props.header && <div className='mx-auto mb-2 max-w-3xl'>
      {props.header}
    </div>}
    {/* Send wraps under the field when the column is too narrow for both, so neither is
        squeezed nor pushed past the edge. */}
    <div className='mx-auto flex max-w-3xl flex-wrap items-end justify-end gap-2'>
      <TextField
        aria-label={t('conversation.composer.label')}
        className='min-w-0 flex-1 basis-48'
        value={prompt}
        onChange={setPrompt}
      >
        <Label className='sr-only'>{t('conversation.composer.label')}</Label>
        <TextArea
          rows={2}
          placeholder={props.placeholder ?? t('conversation.composer.placeholder')}
          onKeyDown={(event) => {
            // Enter sends; Shift+Enter keeps its usual newline. IME composition is left alone.
            if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) {
              event.preventDefault()
              send()
            }
          }}
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
