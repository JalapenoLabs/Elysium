// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Description, Label, TextArea, TextField } from '@heroui/react'
import { LuSendHorizontal } from 'react-icons/lu'

type Props = {
  // Sends the prompt and reports its own failures. Resolves true when the prompt was sent,
  // which clears the field; false keeps what was typed so it can be sent again.
  onSend: (prompt: string) => Promise<boolean>
  // The thread has ended and cannot take prompts.
  isClosed: boolean
  // Shown beside the send button, such as a choice the prompt is sent with.
  accessory?: ReactNode
  placeholder?: string
}

// Sends the next prompt. The turn's progress arrives as live events, so a successful
// send only clears the field.
export function PromptComposer(props: Props) {
  const { t } = useTranslation('coding')
  const [ prompt, setPrompt ] = useState('')
  const [ isSending, setIsSending ] = useState(false)

  const canSend = !props.isClosed && !isSending && prompt.trim().length > 0

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
    <div className='mx-auto flex max-w-3xl items-end gap-2'>
      <TextField
        aria-label={t('conversation.composer.label')}
        className='flex-1'
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
      {props.accessory && <div className='mb-6 shrink-0'>
        {props.accessory}
      </div>}
      <Button
        type='submit'
        className='mb-6'
        isDisabled={!canSend}
        isPending={isSending}
      >
        <LuSendHorizontal className='size-4' aria-hidden />
        <span>{t('conversation.composer.send')}</span>
      </Button>
    </div>
  </form>
}
