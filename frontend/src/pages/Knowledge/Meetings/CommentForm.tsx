// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
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
import { useTextDraft } from '../../../drafts/useTextDraft'
import { keptMentions } from './meetingThreads'

type Props = {
  // What is written is kept for the tab under this name until it is posted, so picking other
  // words, which puts a new thread's form away, or a reload, loses nothing.
  draftName: string
  label: string
  submitLabel: string
  // Resolves whether it was posted; one refused keeps what was written.
  onSubmit: (body: string, mentions: string[]) => Promise<boolean>
  // A form that can be put away, such as a new thread's, has Cancel, which Esc presses too.
  onCancel?: () => void
  focusOnOpen?: boolean
}

// Writing a comment in a meeting's thread, or starting one: the text, with `@` mentions, posted
// with Ctrl+Enter or its button (docs/frontend.md, Hotkeys, Text areas).
export function CommentForm(props: Props) {
  const { t } = useTranslation('meetings')
  const namesById = useAppSelector(selectUserNamesById, shallowEqual)
  const draft = useTextDraft(props.draftName)
  const body = draft.text ?? ''
  const [ mentions, setMentions ] = useState<string[]>([])
  const [ isPosting, setIsPosting ] = useState(false)
  const trimmed = body.trim()

  async function post() {
    if (!trimmed || isPosting) {
      console.debug('CommentForm ignored a post', { isEmpty: !trimmed, isPosting })
      return
    }
    setIsPosting(true)
    try {
      const posted = await props.onSubmit(trimmed, keptMentions(trimmed, mentions, namesById))
      if (posted) {
        draft.forget()
        setMentions([])
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
      value={body}
      onChange={draft.change}
      mentions={mentions}
      onMentionsChange={setMentions}
      onEscape={isPosting
        ? undefined
        : props.onCancel}
      focusOnOpen={props.focusOnOpen}
    />
    <div className='mt-2 level-right gap-2'>
      {props.onCancel && <Button size='sm' variant='ghost' isDisabled={isPosting} onPress={props.onCancel}>
        <span>{t('review.comments.cancel')}</span>
      </Button>}
      <Button type='submit' size='sm' isDisabled={!trimmed} isPending={isPosting}>
        <span>{props.submitLabel}</span>
      </Button>
    </div>
  </Form>
}
