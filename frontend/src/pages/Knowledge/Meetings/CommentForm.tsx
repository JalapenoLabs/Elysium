// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { shallowEqual } from 'react-redux'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectUserNamesById } from '../../../store/usersSlice'

// User interface
import { Button, Form } from '@heroui/react'
import { MentionTextArea } from './MentionTextArea'

// Misc
import { submitOnEnter } from '../../../components/submitOnEnter'
import { forgetSessionDraft, keepSessionDraft, readSessionDraft } from '../../../drafts/sessionDrafts'
import { keptMentions } from './meetingThreads'

type Props = {
  // What is written, and whom it picked to mention, is kept for the tab under this name until it
  // is posted, so picking other words, which puts a new thread's form away, or a reload, loses
  // nothing.
  draftName: string
  label: string
  submitLabel: string
  // Resolves whether it was posted; one refused keeps what was written.
  onSubmit: (body: string, mentions: string[]) => Promise<boolean>
  // A form that can be put away, such as a new thread's, has Cancel, which Esc presses too.
  onCancel?: () => void
  focusOnOpen?: boolean
}

// A comment being written: its text, and the user ids picked to mention as it was typed. The two
// are kept together, since a mention is picked rather than read out of the text.
type CommentDraft = {
  body: string
  mentions: string[]
}

const NO_DRAFT: CommentDraft = { body: '', mentions: []}

// The draft kept under `name`, when storage holds one of this shape; storage is outside the app.
function readCommentDraft(name: string): CommentDraft {
  const stored = readSessionDraft(name)
  if (stored === null) {
    return NO_DRAFT
  }
  if (typeof stored !== 'object' || !('body' in stored) || !('mentions' in stored)) {
    console.debug('A comment draft held something else, so it was left out', { name })
    return NO_DRAFT
  }
  const { body, mentions } = stored
  if (typeof body !== 'string' || !Array.isArray(mentions)) {
    console.debug('A comment draft held something else, so it was left out', { name })
    return NO_DRAFT
  }
  return {
    body,
    mentions: mentions.filter((mention: unknown): mention is string => typeof mention === 'string'),
  }
}

// Writing a comment in a meeting's thread, or starting one: the text, with `@` mentions, posted
// with Ctrl+Enter or its button (docs/frontend.md, Hotkeys, Text areas).
export function CommentForm(props: Props) {
  const { t } = useTranslation('meetings')
  const namesById = useAppSelector(selectUserNamesById, shallowEqual)
  const { draftName } = props
  const [ draft, setDraft ] = useState(() => readCommentDraft(draftName))
  const [ isPosting, setIsPosting ] = useState(false)
  const trimmed = draft.body.trim()

  // Kept as it changes; blank text is never kept, so blanking the field leaves the last words.
  useEffect(() => {
    keepSessionDraft(draftName, draft, [ draft.body ])
  }, [ draftName, draft ])

  async function post() {
    if (!trimmed || isPosting) {
      console.debug('CommentForm ignored a post', { isEmpty: !trimmed, isPosting })
      return
    }
    setIsPosting(true)
    try {
      const posted = await props.onSubmit(trimmed, keptMentions(trimmed, draft.mentions, namesById))
      if (posted) {
        forgetSessionDraft(draftName)
        setDraft(NO_DRAFT)
      }
    }
    finally {
      setIsPosting(false)
    }
  }

  return <Form
    validationBehavior='aria'
    onKeyDownCapture={submitOnEnter}
    onSubmit={(event) => {
      event.preventDefault()
      void post()
    }}
  >
    <MentionTextArea
      label={props.label}
      value={draft.body}
      onChange={(body) => setDraft((current) => ({ ...current, body }))}
      mentions={draft.mentions}
      onMentionsChange={(mentions) => setDraft((current) => ({ ...current, mentions }))}
      onEscape={isPosting
        ? undefined
        : props.onCancel}
      focusOnOpen={props.focusOnOpen}
    />
    <div className='mt-2 flex justify-end gap-2'>
      {props.onCancel && <Button size='sm' variant='ghost' isDisabled={isPosting} onPress={props.onCancel}>
        <span>{t('review.comments.cancel')}</span>
      </Button>}
      <Button type='submit' size='sm' isDisabled={!trimmed} isPending={isPosting}>
        <span>{props.submitLabel}</span>
      </Button>
    </div>
  </Form>
}
