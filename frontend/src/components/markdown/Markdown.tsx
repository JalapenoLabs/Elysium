// Copyright © 2026 Jalapeno Labs

import type { MarkdownProps } from './MarkdownContent'

// Core
import { lazy, Suspense } from 'react'

// User interface
import { Spinner } from '@heroui/react'

// The parser and the highlighter's grammars are heavy, and only the knowledge base's pages read
// Markdown, so they load with the first one shown rather than with the app.
const MarkdownContent = lazy(async () => {
  const module = await import('./MarkdownContent')
  return { default: module.MarkdownContent }
})

// Markdown as it reads: every page, preview, and version shows it through this.
export function Markdown(props: MarkdownProps) {
  return <Suspense fallback={<div className='grid place-items-center py-16'>
    <Spinner />
  </div>}>
    <MarkdownContent {...props} />
  </Suspense>
}
