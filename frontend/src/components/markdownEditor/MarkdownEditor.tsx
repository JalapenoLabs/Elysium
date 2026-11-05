// Copyright © 2026 Jalapeno Labs

import type { MarkdownEditorProps } from './MarkdownEditorContent'

// Core
import { lazy, Suspense } from 'react'

// User interface
import { Spinner } from '@heroui/react'

// Tiptap, ProseMirror, and the Markdown tools are heavy, and only someone editing needs them, so
// they load with the first editor shown, and a person reading a page never fetches them.
const MarkdownEditorContent = lazy(async () => {
  const module = await import('./MarkdownEditorContent')
  return { default: module.MarkdownEditorContent }
})

// A field that edits Markdown as rich text. See docs/frontend.md, The Markdown editor.
export function MarkdownEditor(props: MarkdownEditorProps) {
  return <Suspense fallback={<div className='grid place-items-center py-16'>
    <Spinner />
  </div>}>
    <MarkdownEditorContent {...props} />
  </Suspense>
}
