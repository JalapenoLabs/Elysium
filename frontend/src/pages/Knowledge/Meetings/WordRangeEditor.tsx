// Copyright © 2026 Jalapeno Labs

// Core
import { useCallback, useId, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Description, Form, Label, TextArea, TextField } from '@heroui/react'
import { LuCheck } from 'react-icons/lu'

// Misc
import { submitOnEnter } from '../../../components/submitOnEnter'

// Mirrors `EDIT_TEXT_MAX_CHARACTERS` in api/src/models/meeting_word.rs.
const EDIT_TEXT_MAX_CHARACTERS = 20_000

type Props = {
  // What the range says now, which the field opens with.
  text: string
  // What the correcting agent thought it might say instead, each one click away.
  alternatives: string[]
  // Resolves whether the edit applied; one refused keeps the field open with what was typed.
  onApply: (text: string) => Promise<boolean>
  onCancel: () => void
}

// A range of the transcript opened in place to correct it (docs/meeting-review.md, Clicking,
// selecting, and acting): the agent's alternatives above, when it has any, then the text, applied
// with Ctrl+Enter or Apply, and Esc or Cancel to leave it as it was.
export function WordRangeEditor(props: Props) {
  const { t } = useTranslation('meetings')
  const id = useId()
  const [ text, setText ] = useState(props.text)
  const [ pending, setPending ] = useState<string | null>(null)
  const trimmed = text.trim()
  const canApply = Boolean(trimmed) && trimmed !== props.text.trim() && pending === null

  // Focused as it opens, with what it says selected, so typing replaces it.
  const focusField = useCallback((field: HTMLTextAreaElement | null) => {
    field?.focus()
    field?.select()
  }, [])

  async function apply(next: string) {
    setPending(next)
    try {
      await props.onApply(next)
    }
    finally {
      setPending(null)
    }
  }

  return <Form
    className='rounded-lg border border-(--mark-edit) bg-surface p-3 shadow-sm'
    validationBehavior='aria'
    onKeyDownCapture={(event) => {
      // Esc puts the words back; the page's own Esc must not go up a level.
      if (event.key === 'Escape' && pending === null) {
        event.preventDefault()
        event.stopPropagation()
        props.onCancel()
        return
      }
      submitOnEnter(event)
    }}
    onSubmit={(event) => {
      event.preventDefault()
      if (canApply) {
        void apply(trimmed)
      }
    }}
  >
    {Boolean(props.alternatives.length) && <div className='compact flex flex-wrap items-center gap-2'>
      <span className='text-xs opacity-70'>{t('review.editor.alternatives')}</span>
      {props.alternatives.map((alternative) => <Button
        key={alternative}
        size='sm'
        variant='secondary'
        isPending={pending === alternative}
        isDisabled={pending !== null && pending !== alternative}
        onPress={() => void apply(alternative)}
      >
        <span>{alternative}</span>
      </Button>)}
    </div>}
    <TextField
      id={`meeting-word-editor-${id}`}
      className='w-full'
      value={text}
      maxLength={EDIT_TEXT_MAX_CHARACTERS}
      onChange={setText}
    >
      <Label className='sr-only'>{t('review.editor.label')}</Label>
      <TextArea ref={focusField} rows={2} />
      <Description className='text-xs'>{t('review.editor.hint')}</Description>
    </TextField>
    <div className='mt-2 level-right gap-2'>
      <Button size='sm' variant='ghost' isDisabled={pending !== null} onPress={props.onCancel}>
        <span>{t('review.editor.cancel')}</span>
      </Button>
      <Button
        type='submit'
        size='sm'
        isDisabled={!canApply}
        isPending={pending === trimmed && pending !== null}
      >
        <LuCheck className='size-4' aria-hidden />
        <span>{t('review.editor.apply')}</span>
      </Button>
    </div>
  </Form>
}
