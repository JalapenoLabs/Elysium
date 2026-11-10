// Copyright © 2026 Jalapeno Labs

import type { KeyboardEvent } from 'react'
import type { User } from '../../../api/routes/userRoutes'

// Core
import { useCallback, useId, useLayoutEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectAllUsers } from '../../../store/usersSlice'

// User interface
import { Description, Label, TextArea, TextField } from '@heroui/react'

// Misc
import { useInitialFocus } from '../../../hooks/useInitialFocus'
import { mentionAt, mentionCandidates } from './meetingThreads'

// Mirrors `BODY_MAX_CHARACTERS` in api/src/models/meeting_comment.rs.
export const COMMENT_MAX_CHARACTERS = 20_000

type Props = {
  label: string
  value: string
  onChange: (value: string) => void
  // Everyone picked from the list so far, by user id; the comment keeps those still named in its
  // text (`keptMentions`).
  mentions: string[]
  onMentionsChange: (mentions: string[]) => void
  // What Esc does once no mention list is open, such as putting a new thread away; without it,
  // Esc leaves the field. Either way the page's own Esc never goes up a level from under what is
  // being written.
  onEscape?: () => void
  // Focuses the field as it opens, for a composer the person just asked for.
  focusOnOpen?: boolean
}

// A comment's text area (docs/meeting-review.md, Comments): typing `@` offers the people who can
// be mentioned, and Elysia, which the arrow keys move through and Enter or Tab picks, writing
// `@Name` into the text. Mentions are picked, never read out of the text, so whom a comment
// mentions is never a guess. Esc closes the list before it closes anything else.
export function MentionTextArea(props: Props) {
  const { t } = useTranslation('meetings')
  const id = useId()
  const users = useAppSelector(selectAllUsers)
  const field = useRef<HTMLTextAreaElement | null>(null)
  const [ caret, setCaret ] = useState(0)
  const [ highlighted, setHighlighted ] = useState(0)
  const [ isDismissed, setIsDismissed ] = useState(false)
  // Where to put the caret once a picked name is in the text.
  const pendingCaret = useRef<number | null>(null)

  const typing = isDismissed
    ? null
    : mentionAt(props.value, caret)
  const candidates = typing
    ? mentionCandidates(users, typing.query)
    : []
  const isOpen = Boolean(typing)
  const active = candidates[Math.min(highlighted, candidates.length - 1)]

  // The field is both read for its caret and focused as it opens, with the app's one focus hook.
  const focusOnOpen = useInitialFocus(props.focusOnOpen ?? false)
  const attach = useCallback((element: HTMLTextAreaElement | null) => {
    field.current = element
    focusOnOpen(element)
  }, [ focusOnOpen ])

  useLayoutEffect(() => {
    if (pendingCaret.current === null || !field.current) {
      return
    }
    field.current.setSelectionRange(pendingCaret.current, pendingCaret.current)
    pendingCaret.current = null
  })

  function pick(user: User) {
    if (!typing) {
      console.debug('MentionTextArea was asked to pick with no mention being typed', { userId: user.id })
      return
    }
    const handle = `@${user.name} `
    const next = props.value.slice(0, typing.start) + handle + props.value.slice(caret)
    const nextCaret = typing.start + handle.length
    pendingCaret.current = nextCaret
    setCaret(nextCaret)
    setHighlighted(0)
    props.onChange(next)
    if (!props.mentions.includes(user.id)) {
      props.onMentionsChange([ ...props.mentions, user.id ])
    }
  }

  function onKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    // Esc closes an open list alone; the next press is the field's.
    if (event.key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      if (isOpen) {
        setIsDismissed(true)
        return
      }
      if (props.onEscape) {
        props.onEscape()
        return
      }
      event.currentTarget.blur()
      return
    }
    if (!isOpen) {
      return
    }
    if (!candidates.length || event.ctrlKey || event.metaKey) {
      return
    }
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault()
      const step = event.key === 'ArrowDown'
        ? 1
        : -1
      setHighlighted((current) => (current + step + candidates.length) % candidates.length)
      return
    }
    if ((event.key === 'Enter' || event.key === 'Tab') && !event.shiftKey && active) {
      event.preventDefault()
      pick(active)
    }
  }

  // Reads where the caret is, which says whether a mention is being typed.
  function track(target: HTMLTextAreaElement) {
    setCaret(target.selectionStart)
  }

  const listId = `${id}-mentions`
  return <div className='relative'>
    <TextField
      id={`meeting-comment-${id}`}
      className='w-full'
      value={props.value}
      maxLength={COMMENT_MAX_CHARACTERS}
      onChange={(value) => {
        props.onChange(value)
        // Where typing left the caret, which says whether a mention is being typed.
        setCaret(field.current?.selectionStart ?? value.length)
        setIsDismissed(false)
        setHighlighted(0)
      }}
    >
      <Label className='sr-only'>{props.label}</Label>
      <TextArea
        ref={attach}
        rows={2}
        aria-controls={isOpen
          ? listId
          : undefined}
        aria-activedescendant={isOpen && active
          ? `${listId}-${active.id}`
          : undefined}
        onKeyDown={onKeyDown}
        onSelect={(event) => track(event.currentTarget)}
        onKeyUp={(event) => track(event.currentTarget)}
        onClick={(event) => track(event.currentTarget)}
        onBlur={() => setIsDismissed(true)}
        onFocus={(event) => {
          setIsDismissed(false)
          track(event.currentTarget)
        }}
      />
      <Description className='text-xs'>{t('review.comments.hint')}</Description>
    </TextField>

    {isOpen && <ul
      id={listId}
      role='listbox'
      aria-label={t('review.comments.mentionsLabel')}
      className={[
        'absolute top-full left-0 z-30 mt-1 w-full max-w-xs overflow-hidden rounded-lg border',
        'border-border bg-overlay py-1 text-sm shadow-lg',
      ].join(' ')}
    >
      {candidates.length
        ? candidates.map((user) => <li
          key={user.id}
          id={`${listId}-${user.id}`}
          role='option'
          aria-selected={user.id === active?.id}
          className={[
            'cursor-pointer truncate px-3 py-1.5',
            user.id === active?.id
              ? 'bg-default'
              : '',
          ].join(' ')}
          // Before the field loses focus to the press, so the pick lands in the text.
          onMouseDown={(event) => {
            event.preventDefault()
            pick(user)
          }}
        >{user.name}</li>)
        : <li className='px-3 py-1.5 opacity-70'>{t('review.comments.noOneToMention')}</li>}
    </ul>}
  </div>
}
