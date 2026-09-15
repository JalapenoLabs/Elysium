// Copyright © 2026 Jalapeno Labs

import type { KeyboardEvent } from 'react'

// Core
import { useEffect, useRef, useState } from 'react'

type Props = {
  value: string
  // Receives the trimmed new value, only when it changed. Throwing keeps the editor open
  // on the attempted value, so the caller reports the failure and the user can fix it.
  onSave: (value: string) => Promise<void>
  // Describes what is being edited, for assistive technology.
  label: string
  // Shown in place of an empty value.
  placeholder?: string
  isRequired?: boolean
  maxLength?: number
  // A text area that grows with its content, for longer text. Enter adds a line;
  // Ctrl or Cmd with Enter saves.
  isMultiline?: boolean
  // Typography, shared by the text and its editor so switching does not shift the layout.
  className: string
}

const DISPLAY_CLASS_NAME = [
  '-mx-1 inline-block max-w-full cursor-text rounded-md px-1 text-left',
  'transition-colors hover:bg-surface-secondary',
  'focus-visible:ring-2 focus-visible:ring-focus focus-visible:outline-none',
].join(' ')

const EDITOR_CLASS_NAME = '-mx-1 w-full rounded-md bg-transparent px-1 ring-2 ring-focus outline-none'

// Text that becomes its own editor when clicked. The text is only as wide as it is, so the
// hover highlight hugs it; the editor takes the full width to leave room for typing.
// Enter (or Ctrl+Enter when multi-line) and clicking away save; Escape cancels. An empty
// value is refused when required.
export function InlineEditableText(props: Props) {
  const [ isEditing, setIsEditing ] = useState(false)
  const [ draft, setDraft ] = useState(props.value)
  const [ isSaving, setIsSaving ] = useState(false)
  const editorRef = useRef<HTMLInputElement & HTMLTextAreaElement>(null)

  useEffect(() => {
    if (!isEditing || !editorRef.current) {
      return
    }
    const editor = editorRef.current
    editor.focus()
    // A name is usually replaced whole; a description is usually added to.
    if (props.isMultiline) {
      editor.setSelectionRange(editor.value.length, editor.value.length)
      return
    }
    editor.select()
  }, [ isEditing, props.isMultiline ])

  // A text area sized to its content never scrolls inside the page.
  useEffect(() => {
    const editor = editorRef.current
    if (!isEditing || !props.isMultiline || !editor) {
      return
    }
    editor.style.height = 'auto'
    editor.style.height = `${editor.scrollHeight}px`
  }, [ isEditing, props.isMultiline, draft ])

  function startEditing() {
    setDraft(props.value)
    setIsEditing(true)
  }

  async function save() {
    // Disabling the editor while saving blurs it, which would save a second time.
    if (isSaving) {
      return
    }
    const trimmed = draft.trim()
    if (trimmed === props.value.trim()) {
      setIsEditing(false)
      return
    }
    if (props.isRequired && !trimmed) {
      console.debug('InlineEditableText refused an empty required value', { label: props.label })
      setIsEditing(false)
      return
    }

    setIsSaving(true)
    try {
      await props.onSave(trimmed)
      setIsEditing(false)
    }
    catch (error) {
      console.debug('InlineEditableText kept editing: saving failed', { error, label: props.label })
      editorRef.current?.focus()
    }
    finally {
      setIsSaving(false)
    }
  }

  function onKeyDown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault()
      setIsEditing(false)
      return
    }
    const savesOnEnter = !props.isMultiline || event.metaKey || event.ctrlKey
    if (event.key === 'Enter' && savesOnEnter) {
      event.preventDefault()
      void save()
    }
  }

  const editorClassName = [ props.className, EDITOR_CLASS_NAME ].join(' ')

  if (isEditing) {
    const sharedProps = {
      'ref': editorRef,
      'aria-label': props.label,
      'value': draft,
      'maxLength': props.maxLength,
      'disabled': isSaving,
      'placeholder': props.placeholder,
      'className': editorClassName,
      'onKeyDown': onKeyDown,
      'onBlur': () => void save(),
    }
    return props.isMultiline
      ? <textarea
        {...sharedProps}
        rows={1}
        className={`${editorClassName} resize-none overflow-hidden`}
        onChange={(event) => setDraft(event.currentTarget.value)}
      />
      : <input
        {...sharedProps}
        onChange={(event) => setDraft(event.currentTarget.value)}
      />
  }

  return <button
    type='button'
    title={props.label}
    className={`${props.className} ${DISPLAY_CLASS_NAME}`}
    onClick={startEditing}
  >{
    props.value || <span className='opacity-50'>{props.placeholder}</span>
  }</button>
}
