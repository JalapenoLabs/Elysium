// Copyright © 2026 Jalapeno Labs

import type { ElysiaTurn } from '../api/routes/elysiaRoutes'

// Core
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch, useAppSelector } from '../store/hooks'
import { elysiaMessageCreated, elysiaMessagesLoaded, selectElysiaMessages } from '../store/elysiaSlice'

// User interface
import { Button, Form, Label, Spinner, TextArea, TextField, toast } from '@heroui/react'
import { LuSendHorizontal } from 'react-icons/lu'

// Misc
import { listElysiaMessages, sendElysiaMessage } from '../api/routes/elysiaRoutes'
import { useElysiaMessagesLoader } from '../hooks/useServerData'
import { useUserNames } from '../hooks/useUserNames'

// A message's length, as the API bounds it.
const MESSAGE_MAX_CHARACTERS = 20_000

type Props = {
  // Her queued and running turns, to show that a message is waiting or being worked on.
  activeTurns: ElysiaTurn[]
}

// The conversation with Elysia, oldest first, with the composer below. Her reply arrives as
// an `elysiaMessage.created` event when her turn ends.
export function ElysiaConversation(props: Props) {
  const { t, i18n } = useTranslation('elysia')
  const dispatch = useAppDispatch()
  const status = useElysiaMessagesLoader()
  const messages = useAppSelector(selectElysiaMessages)
  const userNames = useUserNames()
  const [ draft, setDraft ] = useState('')
  const [ isSending, setIsSending ] = useState(false)
  const [ isLoadingEarlier, setIsLoadingEarlier ] = useState(false)
  const [ hasEarlier, setHasEarlier ] = useState(true)
  const endRef = useRef<HTMLDivElement>(null)
  const timeFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'short', timeStyle: 'short' })

  const lastMessageId = messages.at(-1)?.id
  useEffect(() => {
    endRef.current?.scrollIntoView({ block: 'end' })
  }, [ lastMessageId ])

  let pendingNote: string | null = null
  for (const turn of props.activeTurns) {
    if (turn.triggerKind !== 'message' && turn.triggerKind !== 'answer') {
      continue
    }
    if (turn.status === 'running') {
      pendingNote = t('conversation.thinking')
      break
    }
    pendingNote = t('conversation.queued')
  }

  async function send() {
    const body = draft.trim()
    if (!body) {
      console.debug('ElysiaConversation ignored an empty message')
      return
    }
    setIsSending(true)
    try {
      const response = await sendElysiaMessage(body)
      dispatch(elysiaMessageCreated(response.message))
      setDraft('')
    }
    catch (error) {
      console.debug('ElysiaConversation failed to send a message', { error })
      toast.danger(t('composer.failed'))
    }
    finally {
      setIsSending(false)
    }
  }

  async function loadEarlier() {
    const oldest = messages[0]
    if (!oldest) {
      console.debug('ElysiaConversation has no message to load earlier ones before')
      return
    }
    setIsLoadingEarlier(true)
    try {
      const response = await listElysiaMessages(oldest.id)
      dispatch(elysiaMessagesLoaded(response.messages))
      setHasEarlier(Boolean(response.messages.length))
    }
    catch (error) {
      console.debug('ElysiaConversation failed to load earlier messages', { error })
      toast.danger(t('conversation.loadFailed'))
    }
    finally {
      setIsLoadingEarlier(false)
    }
  }

  return <div className='flex min-h-0 flex-1 flex-col'>
    <div className='min-h-0 flex-1 overflow-y-auto pr-1'>
      {status === 'loading' && <div className='grid place-items-center py-6'>
        <Spinner size='sm' />
      </div>}
      {status === 'failed' && <p className='compact text-sm text-danger'>{t('conversation.loadFailed')}</p>}
      {status === 'loaded' && !messages.length && <p className='compact text-sm opacity-70'>{
        t('conversation.empty')
      }</p>}
      {Boolean(messages.length) && hasEarlier && <div className='compact flex justify-center'>
        <Button size='sm' variant='ghost' isPending={isLoadingEarlier} onPress={() => void loadEarlier()}>
          <span>{t('conversation.earlier')}</span>
        </Button>
      </div>}
      <ol className='flex flex-col gap-3'>{
        messages.map((message) => <li
          key={message.id}
          className={message.fromElysia
            ? 'mr-6 rounded-xl bg-surface p-3'
            : 'ml-6 rounded-xl bg-default p-3'}
        >
          <div className='mb-1 text-xs'>
            <span className='font-semibold'>{
              message.fromElysia
                ? t('name')
                : userNames[message.createdBy] ?? t('conversation.you')
            }</span>
            <span className='ml-2 opacity-60'>{timeFormatter.format(new Date(message.createdAt))}</span>
          </div>
          <p className='text-sm whitespace-pre-line'>{message.body}</p>
        </li>)
      }</ol>
      {pendingNote && <p className='mt-3 text-xs opacity-60'>{pendingNote}</p>}
      <div ref={endRef} />
    </div>

    <Form
      className='mt-3 shrink-0'
      validationBehavior='aria'
      onSubmit={(event) => {
        event.preventDefault()
        void send()
      }}
    >
      <TextField
        className='w-full'
        value={draft}
        maxLength={MESSAGE_MAX_CHARACTERS}
        onChange={setDraft}
      >
        <Label className='sr-only'>{t('composer.label')}</Label>
        <TextArea
          rows={2}
          placeholder={t('composer.label')}
          onKeyDown={(event) => {
            // Enter sends; Shift with Enter adds a line, as in chat.
            if (event.key === 'Enter' && !event.shiftKey) {
              event.preventDefault()
              void send()
            }
          }}
        />
      </TextField>
      <div className='mt-2 flex justify-end'>
        <Button type='submit' size='sm' isDisabled={!draft.trim()} isPending={isSending}>
          <LuSendHorizontal className='size-4' aria-hidden />
          <span>{t('composer.send')}</span>
        </Button>
      </div>
    </Form>
  </div>
}
