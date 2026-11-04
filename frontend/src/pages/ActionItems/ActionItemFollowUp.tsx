// Copyright © 2026 Jalapeno Labs

import type { ActionItem } from '../../api/routes/actionItemRoutes'
import type { Draft } from '../../api/routes/draftRoutes'
import type { PersonPromise } from '../../api/routes/promiseRoutes'

// Core
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { selectIsCompanyMember, selectMe } from '../../store/authSlice'
import { draftUpserted, selectDraftById } from '../../store/draftsSlice'
import { useAppDispatch, useAppSelector } from '../../store/hooks'
import { selectPromiseByFollowUpItemId } from '../../store/promisesSlice'

// User interface
import { Button, Description, Form, Label, Link, TextArea, TextField } from '@heroui/react'
import { LuExternalLink, LuPencil, LuSend, LuX } from 'react-icons/lu'
import { ReportIssueButton } from '../../components/ReportIssueButton'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { sendDraft, updateDraft } from '../../api/routes/draftRoutes'
import { submitOnEnter } from '../../components/submitOnEnter'
import { useTextDraft } from '../../drafts/useTextDraft'
import { formatDateTime } from '../../format/dates'
import { useDraftLoader, useFollowUpPromiseLoader } from '../../hooks/useServerData'
import { useUserNames } from '../../hooks/useUserNames'
import { toast } from '../../toast/toast'
import { useActionItemActions } from './useActionItemActions'

// A follow-up's length, as the API bounds a draft.
const FOLLOW_UP_MAX_CHARACTERS = 20_000

type Props = {
  item: ActionItem
}

// The follow-up an item is, when a Project Manager drafted it to keep a promise: who it is for
// and where it goes, the message in the person's voice, and Send, Edit, and Dismiss. Sending
// posts it through the connector the promise was made in and resolves the item; nothing is sent
// without it. Anyone else in the company sees the promise, and that it is its person's to send.
// An item that follows up no promise shows nothing.
export function ActionItemFollowUp(props: Props) {
  const isMember = useAppSelector(selectIsCompanyMember)
  useFollowUpPromiseLoader(props.item.id, isMember)
  const promise = useAppSelector((state) => selectPromiseByFollowUpItemId(state, props.item.id))

  if (!promise) {
    return null
  }
  return <FollowUpCard item={props.item} promise={promise} />
}

type CardProps = {
  item: ActionItem
  promise: PersonPromise
}

function FollowUpCard(props: CardProps) {
  const { t } = useTranslation([ 'actionItems', 'common' ])
  const dispatch = useAppDispatch()
  const me = useAppSelector(selectMe)
  const userNames = useUserNames()
  const itemActions = useActionItemActions()
  const promise = props.promise
  const madeIn = promise.madeIn
  const isPersons = Boolean(madeIn && me?.user.id === madeIn.personId)

  // Only its person reads the draft; anyone else is told it is theirs to send.
  const draftId = isPersons
    ? promise.draftId
    : null
  const draftStatus = useDraftLoader(draftId)
  const draft = useAppSelector((state) => draftId
    ? selectDraftById(state, draftId)
    : undefined)

  const [ isEditing, setIsEditing ] = useState(false)
  const [ isSending, setIsSending ] = useState(false)
  const [ isSaving, setIsSaving ] = useState(false)
  // What the person typed while editing, kept for the tab until it is saved or put back.
  const typedDraft = useTextDraft(`followUp.${promise.id}`)
  const editorRef = useRef<HTMLTextAreaElement>(null)

  useEffect(() => {
    if (isEditing) {
      editorRef.current?.focus()
    }
  }, [ isEditing ])

  const isOwed = props.item.state === 'inbox' || props.item.state === 'open'
  const isSent = draft?.state === 'sent'
  const canAct = isPersons && Boolean(draft) && isOwed && !isSent
  const edited = typedDraft.text ?? draft?.body ?? ''

  async function send(current: Draft) {
    if (isSending) {
      console.debug('ActionItemFollowUp ignored a second Send while one was under way', { draftId: current.id })
      return
    }
    setIsSending(true)
    try {
      const response = await sendDraft(current.id)
      dispatch(draftUpserted(response.draft))
      if (response.draft.state === 'sent') {
        toast.success(t('followUp.sent', { recipient: promise.recipient }))
      }
    }
    catch (error) {
      console.debug('ActionItemFollowUp failed to send a follow-up', { error, draftId: current.id })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
    finally {
      setIsSending(false)
    }
  }

  async function save(current: Draft) {
    const body = edited.trim()
    if (!body || isSaving) {
      console.debug('ActionItemFollowUp ignored a save', { isEmpty: !body, isSaving, draftId: current.id })
      return
    }
    setIsSaving(true)
    try {
      const response = await updateDraft(current.id, body)
      dispatch(draftUpserted(response.draft))
      typedDraft.forget()
      setIsEditing(false)
      toast.success(t('followUp.saved'))
    }
    catch (error) {
      console.debug('ActionItemFollowUp failed to save a follow-up', { error, draftId: current.id })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
    finally {
      setIsSaving(false)
    }
  }

  function stopEditing() {
    typedDraft.forget()
    setIsEditing(false)
  }

  const personName = madeIn
    ? userNames[madeIn.personId] ?? t('common:table.someone')
    : t('common:table.someone')

  return <section className='relaxed'>
    <h2 className='compact text-lg font-semibold'>{t('followUp.heading')}</h2>
    <div className='compact flex flex-wrap items-center gap-x-3 gap-y-1 text-sm'>
      <span>{madeIn
        ? t('followUp.toIn', { recipient: promise.recipient, place: madeIn.title })
        : t('followUp.to', { recipient: promise.recipient })}</span>
      {madeIn?.url && <Link href={madeIn.url} target='_blank' rel='noreferrer' className='gap-1 text-sm'>
        <span>{t('followUp.openConversation')}</span>
        <LuExternalLink className='size-3.5' aria-hidden />
      </Link>}
    </div>
    <p className='compact text-sm opacity-70'>{isPersons
      ? t('followUp.promisedByYou', { promise: promise.summary })
      : t('followUp.promisedBy', { name: personName, promise: promise.summary })}</p>

    {!isPersons && <p className='text-sm opacity-80'>{t('followUp.personsToSend', { name: personName })}</p>}
    {isPersons && !draft && <p className='text-sm opacity-70'>{
      draftStatus === 'failed'
        ? t('followUp.loadError')
        : t('followUp.loading')
    }</p>}

    {draft && isEditing && <Form
      validationBehavior='aria'
      onKeyDownCapture={submitOnEnter}
      onSubmit={(event) => {
        event.preventDefault()
        void save(draft)
      }}
    >
      <TextField
        id={`follow-up-${promise.id}-text`}
        className='compact w-full'
        value={edited}
        maxLength={FOLLOW_UP_MAX_CHARACTERS}
        isDisabled={isSaving}
        onChange={typedDraft.change}
      >
        <Label className='sr-only'>{t('followUp.label')}</Label>
        <TextArea
          ref={editorRef}
          rows={4}
          onKeyDown={(event) => {
            // Escape puts back what was there, as it does in every text edited in place.
            if (event.key === 'Escape') {
              stopEditing()
            }
          }}
        />
        <Description>{t('followUp.hint')}</Description>
      </TextField>
      <div className='flex flex-wrap gap-2'>
        <Button type='submit' isPending={isSaving} isDisabled={!edited.trim()}>
          <span>{t('followUp.save')}</span>
        </Button>
        <Button variant='ghost' isDisabled={isSaving} onPress={stopEditing}>
          <span>{t('common:actions.cancel')}</span>
        </Button>
      </div>
    </Form>}

    {draft && !isEditing && <>
      <blockquote className='compact rounded-lg border border-separator bg-surface p-3 text-sm whitespace-pre-line'>{
        draft.body
      }</blockquote>
      {isSent && draft.sentAt && <p className='text-sm opacity-70'>{
        t('followUp.sentAt', { time: formatDateTime(draft.sentAt) })
      }</p>}
      {draft.state === 'failed' && draft.lastError && <div className='compact flex items-start gap-2'>
        <p className='text-sm text-danger'>{t('followUp.failed', { reason: draft.lastError })}</p>
        <ReportIssueButton
          isIconOnly
          subject={{
            kind: 'error',
            area: `drafts.send.${madeIn?.sourceKind ?? 'follow-up'}`,
            message: draft.lastError,
          }}
        />
      </div>}
      {canAct && <div className='flex flex-wrap gap-2'>
        <Button isPending={isSending} onPress={() => void send(draft)}>
          <LuSend className='size-4' aria-hidden />
          <span>{isSending
            ? t('followUp.sending')
            : t('followUp.send')}</span>
        </Button>
        <Button variant='secondary' isDisabled={isSending} onPress={() => setIsEditing(true)}>
          <LuPencil className='size-4' aria-hidden />
          <span>{t('followUp.edit')}</span>
        </Button>
        <Button
          variant='ghost'
          isDisabled={isSending}
          onPress={() => void itemActions.transition(props.item, 'dismiss')}
        >
          <LuX className='size-4' aria-hidden />
          <span>{t('followUp.dismiss')}</span>
        </Button>
      </div>}
    </>}
  </section>
}
