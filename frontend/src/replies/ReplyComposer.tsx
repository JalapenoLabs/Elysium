// Copyright © 2026 Jalapeno Labs

import type { ConnectorReplies } from '../api/routes/connectorRoutes'
import type { ReplyPlace } from '../api/routes/draftRoutes'
import type { TimelineRecord } from '../api/routes/timelineRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch, useAppSelector } from '../store/hooks'
import { selectMe } from '../store/authSlice'
import { draftRemoved, draftUpserted, selectUnsentRecordDrafts } from '../store/draftsSlice'
import { selectAllMailAccounts } from '../store/mailAccountsSlice'

// User interface
import { Button, Description, Form, Label, Link, TextArea, TextField } from '@heroui/react'
import { LuSend } from 'react-icons/lu'
import { OptionSelect } from '../components/OptionSelect'
import { ReportIssueButton } from '../components/ReportIssueButton'

// Misc
import { getApiErrorMessage } from '../api/errors'
import { createReplyDraft, deleteDraft, sendDraft, updateDraft } from '../api/routes/draftRoutes'
import { useMailAccountsLoader, useRecordDraftsLoader } from '../hooks/useServerData'
import { UrlTree } from '../urls'
import {
  forgetReply,
  mailRecipients,
  placeLabelKeys,
  readSavedReply,
  saveReply,
  sendingMailboxes,
} from './replyPresentation'
import { toast } from '../toast/toast'

// A message's length, as the API bounds a draft.
const REPLY_MAX_CHARACTERS = 20_000

type Props = {
  record: TimelineRecord
  // How the record's kind is answered; never `never`.
  replies: Exclude<ConnectorReplies, { through: 'never' }>
}

// A reply to one record, sent as the person through what answers its kind: the connector itself
// for a chat, or one of their mailboxes for mail. It continues the record's newest unsent draft,
// so a send that failed keeps its text here with the reason, ready to send again. What is typed
// is kept for the tab until it is sent or discarded.
export function ReplyComposer(props: Props) {
  const { t } = useTranslation([ 'timeline', 'common' ])
  const dispatch = useAppDispatch()
  const record = props.record
  const draftsStatus = useRecordDraftsLoader(record.id)
  // Only mail needs them, but a hook cannot wait on the kind; the list is small and shared.
  useMailAccountsLoader()
  const drafts = useAppSelector((state) => selectUnsentRecordDrafts(state, record.id))
  const me = useAppSelector(selectMe)
  const mailAccounts = useAppSelector(selectAllMailAccounts)
  const draft = drafts[0] ?? null
  // `null` until the person types, so the box shows a stored draft's text once it loads.
  const [ typed, setTyped ] = useState<string | null>(() => readSavedReply(record.id))
  // The kind's first place unless the person picks another; none lets the API choose.
  const [ place, setPlace ] = useState<ReplyPlace | null>(props.replies.through === 'connector'
    ? props.replies.places[0] ?? null
    : null)
  const [ chosenMailboxId, setChosenMailboxId ] = useState<string | null>(null)
  const [ isSending, setIsSending ] = useState(false)

  const text = typed ?? draft?.body ?? ''
  const mailboxes = sendingMailboxes(mailAccounts, me?.user.id ?? null)
  const mailboxId = chosenMailboxId ?? (mailboxes.length === 1
    ? mailboxes[0].id
    : null)
  const isMail = props.replies.through === 'mailbox'

  function type(value: string) {
    setTyped(value)
    saveReply(record.id, value)
  }

  async function send() {
    const body = text.trim()
    if (!body || isSending) {
      console.debug('ReplyComposer ignored a send', { isEmpty: !body, isSending, recordId: record.id })
      return
    }
    setIsSending(true)
    try {
      let current = draft
      if (current && current.body !== body) {
        current = (await updateDraft(current.id, body)).draft
        dispatch(draftUpserted(current))
      }
      if (!current) {
        current = (await createReplyDraft({
          recordId: record.id,
          place: place ?? undefined,
          mailAccountId: isMail
            ? mailboxId ?? undefined
            : undefined,
          body,
        })).draft
        dispatch(draftUpserted(current))
      }
      const response = await sendDraft(current.id)
      dispatch(draftUpserted(response.draft))
      if (response.draft.state === 'sent') {
        setTyped(null)
        forgetReply(record.id)
        toast.success(t('reply.sent'))
      }
    }
    catch (error) {
      console.debug('ReplyComposer failed to send a reply', { error, recordId: record.id })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
    finally {
      setIsSending(false)
    }
  }

  async function discard() {
    if (!draft) {
      setTyped(null)
      forgetReply(record.id)
      return
    }
    try {
      await deleteDraft(draft.id)
      dispatch(draftRemoved(draft.id))
      setTyped(null)
      forgetReply(record.id)
    }
    catch (error) {
      console.debug('ReplyComposer failed to discard a draft', { error, draftId: draft.id })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
  }

  if (isMail && !draft && !mailboxes.length) {
    return <p className='text-sm opacity-80'>
      <span>{t('reply.noMailbox')} </span>
      <Link href={UrlTree.settingsEmail} className='text-link'>{t('reply.connectMailbox')}</Link>
    </p>
  }

  const recipient = draft && isMail
    ? mailRecipients(draft.target)
    : record.actor
  const canChoosePlace = !draft
    && props.replies.through === 'connector'
    && props.replies.places.length > 1
  const canChooseMailbox = !draft && isMail && mailboxes.length > 1

  return <Form
    validationBehavior='aria'
    onSubmit={(event) => {
      event.preventDefault()
      void send()
    }}
  >
    {(canChoosePlace || canChooseMailbox || isMail) && <div className='compact flex flex-wrap items-end gap-3'>
      {props.replies.through === 'connector' && canChoosePlace && <OptionSelect
        className='w-56'
        label={t('reply.place')}
        value={place ?? ''}
        options={props.replies.places.map((option) => ({ id: option, label: t(placeLabelKeys[option]) }))}
        onChange={(value) => setPlace(value === 'conversation'
          ? 'conversation'
          : 'thread')}
      />}
      {canChooseMailbox && <OptionSelect
        className='w-72'
        label={t('reply.from')}
        value={mailboxId ?? ''}
        options={mailboxes.map((mailbox) => ({ id: mailbox.id, label: mailbox.address }))}
        onChange={setChosenMailboxId}
      />}
      {isMail && <p className='text-sm opacity-70'>{t('reply.to', { recipient })}</p>}
    </div>}

    <TextField
      className='compact w-full'
      value={text}
      maxLength={REPLY_MAX_CHARACTERS}
      isDisabled={isSending}
      onChange={type}
    >
      <Label className='sr-only'>{t('reply.label')}</Label>
      <TextArea
        rows={3}
        placeholder={t('reply.placeholder')}
        onKeyDown={(event) => {
          // Enter adds a line; Ctrl or Cmd with Enter sends, as comments post.
          if (event.key === 'Enter' && (event.ctrlKey || event.metaKey)) {
            event.preventDefault()
            void send()
          }
        }}
      />
      <Description>{t('reply.hint')}</Description>
    </TextField>

    {draftsStatus === 'failed' && <p className='compact text-sm text-danger'>{t('reply.loadError')}</p>}
    {draft?.state === 'failed' && draft.lastError && <div className='compact flex items-start gap-2'>
      <p className='text-sm text-danger'>{t('reply.failed', { reason: draft.lastError })}</p>
      <ReportIssueButton
        isIconOnly
        subject={{
          kind: 'error',
          area: `drafts.send.${record.sourceKind}`,
          message: draft.lastError,
        }}
      />
    </div>}

    <div className='flex flex-wrap gap-2'>
      <Button type='submit' isPending={isSending} isDisabled={!text.trim() || (isMail && !draft && !mailboxId)}>
        <LuSend className='size-4' aria-hidden />
        <span>{isSending
          ? t('reply.sending')
          : t('reply.send')}</span>
      </Button>
      {(draft || typed) && <Button variant='ghost' isDisabled={isSending} onPress={() => void discard()}>
        <span>{t('reply.discard')}</span>
      </Button>}
    </div>
  </Form>
}
