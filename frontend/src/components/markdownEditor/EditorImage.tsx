// Copyright © 2026 Jalapeno Labs

import type { NodeViewProps } from '@tiptap/react'

// User interface
import { NodeViewWrapper } from '@tiptap/react'
import { MarkdownImage } from '../markdown/MarkdownImage'

// An image in the editor keeps the page's rule: only Elysium's own loads, and any other is a link
// (`MarkdownImage`), so editing a page tells no third party who opened it.
export function EditorImage(props: NodeViewProps) {
  const { src, alt, title } = props.node.attrs

  return <NodeViewWrapper as='span' className='markdown-editor-image'>
    <MarkdownImage src={src} alt={alt ?? undefined} title={title ?? undefined} />
  </NodeViewWrapper>
}
