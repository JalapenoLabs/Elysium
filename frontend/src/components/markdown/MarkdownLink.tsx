// Copyright © 2026 Jalapeno Labs

import type { ComponentProps } from 'react'
import type { ExtraProps } from 'react-markdown'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Link } from '@heroui/react'
import { LuExternalLink } from 'react-icons/lu'

// Misc
import { readMarkdownLink } from './markdownUrls'

type Props = ComponentProps<'a'> & ExtraProps

// A link in a page's text. One that stays in Elysium routes in the app, one that leaves opens in
// a new tab and says so, and one within the document stays a plain anchor, since the browser
// scrolls to it and the router would not. An address `urlTransform` blanked keeps its text.
export function MarkdownLink(props: Props) {
  const { t } = useTranslation('common')
  const { children } = props
  const target = readMarkdownLink(props.href)

  if (target.kind === 'none') {
    return <span>{children}</span>
  }
  if (target.kind === 'fragment') {
    // A footnote's mark and its way back name each other by these.
    return <a
      id={props.id}
      href={target.href}
      aria-label={props['aria-label']}
      aria-describedby={props['aria-describedby']}
    >{
        children
      }</a>
  }
  if (target.kind === 'internal') {
    return <Link href={target.href}>
      <span title={props.title}>{children}</span>
    </Link>
  }
  return <Link href={target.href} target='_blank' rel='noreferrer'>
    <span title={props.title}>{children}</span>
    <LuExternalLink className='markdown-external' aria-label={t('markdown.opensInNewTab')} />
  </Link>
}
