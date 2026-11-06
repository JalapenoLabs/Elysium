// Copyright © 2026 Jalapeno Labs

import type { ComponentProps } from 'react'
import type { ExtraProps } from 'react-markdown'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { CopyButton } from '../CopyButton'

type Props = ComponentProps<'pre'> & ExtraProps

type HastChild = NonNullable<ExtraProps['node']>['children'][number]

// A code block's text is split across the highlighter's spans.
function readText(node: HastChild): string {
  if (node.type === 'text') {
    return node.value
  }
  if (node.type === 'element') {
    return node.children
      .map(readText)
      .join('')
  }
  return ''
}

// A code block, which scrolls sideways on its own so a long line never widens the page, with a
// button to copy it.
export function MarkdownCodeBlock(props: Props) {
  const { t } = useTranslation('common')
  const text = props.node?.children
    .map(readText)
    .join('')
    .trimEnd()

  return <div className={`markdown-code-block ${props.className ?? ''}`}>
    <pre>{props.children}</pre>
    {text && <div className='markdown-code-copy'>
      <CopyButton value={text} label={t('markdown.copyCode')} />
    </div>}
  </div>
}
