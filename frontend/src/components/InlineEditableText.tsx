// Copyright © 2026 Jalapeno Labs

import type { KeyboardEvent } from 'react'

// Core
import { useEffect, useRef, useState } from 'react'

type Props = {
  // The value as the page's form holds it. Edits change the form; the page's Save keeps them.
  value: string
  onChange: (value: string) => void
  // Describes what is being edited, for assistive technology.
  label: string
  // Shown in place of an empty value.
  placeholder?: string
  isRequired?: boolean
  maxLength?: number
  // A text area that grows with its content, for longer text. Enter adds a line;
  // Ctrl or Cmd with Enter finishes.
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

// A field of the page's form that reads as text until clicked, then edits in place. The text
// is only as wide as it is, so the hover highlight hugs it; the editor takes the full width to
// leave room for typing. Every keystroke changes the form, so Save and Ctrl+S always see what
// is typed. Enter (or Ctrl+Enter when multi-line) and clicking away finish editing; Escape
// puts back what was there when editing began. A required value left empty is put back too.
export function InlineEditableText(props: Props) {
  const [ isEditing, setIsEditing ] = useState(false)
  const [ valueBeforeEditing, setValueBeforeEditing ] = useState(props.value)
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
  }, [ isEditing, props.isMultiline, props.value ])

  function startEditing() {
    setValueBeforeEditing(props.value)
    setIsEditing(true)
  }

  function finishEditing() {
    setIsEditing(false)
    const trimmed = props.value.trim()
    if (props.isRequired && !trimmed) {
      console.debug('InlineEditableText put back an empty required value', { label: props.label })
      props.onChange(valueBeforeEditing)
      return
    }
    if (trimmed !== props.value) {
      props.onChange(trimmed)
    }
  }

  function onKeyDown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      // Marked, so the page does not also take Esc as leaving.
      event.preventDefault()
      props.onChange(valueBeforeEditing)
      setIsEditing(false)
      return
    }
    const finishesOnEnter = !props.isMultiline || event.metaKey || event.ctrlKey
    if (event.key === 'Enter' && finishesOnEnter) {
      // Enter finishes the field rather than submitting the page's form.
      event.preventDefault()
      finishEditing()
    }
  }

  const editorClassName = [ props.className, EDITOR_CLASS_NAME ].join(' ')

  if (isEditing) {
    const sharedProps = {
      'ref': editorRef,
      'aria-label': props.label,
      'value': props.value,
      'maxLength': props.maxLength,
      'placeholder': props.placeholder,
      'className': editorClassName,
      'onKeyDown': onKeyDown,
      'onBlur': finishEditing,
    }
    return props.isMultiline
      ? <textarea
        {...sharedProps}
        rows={1}
        className={`${editorClassName} resize-none overflow-hidden`}
        onChange={(event) => props.onChange(event.currentTarget.value)}
      />
      : <input
        {...sharedProps}
        onChange={(event) => props.onChange(event.currentTarget.value)}
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
