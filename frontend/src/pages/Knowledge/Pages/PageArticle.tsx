// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'
import type { MarkdownRange } from '../../../components/markdown/rehypeMarkPassage'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Markdown } from '../../../components/markdown/Markdown'

type Props = {
  markdown: string
  // The passage a citation opened the page at, marked and brought into view.
  markedRange?: MarkdownRange | null
  // Who wrote it, and when: the current version's, or an old one's when reading its history.
  byline: ReactNode
  // Each of these fills its place when given, and takes no space when not.
  labels?: ReactNode
  // A learning's category and evidence, on a page that is one.
  learning?: ReactNode
  attachments?: ReactNode
  childPages?: ReactNode
}

// A page as it reads, top to bottom. Pages that hold others often say nothing themselves, so with
// no text its child pages come straight after the byline; a page with neither says it is empty.
export function PageArticle(props: Props) {
  const { t } = useTranslation('knowledge')

  return <article className='max-w-4xl'>
    <div className='relaxed flex flex-wrap items-center gap-x-3 gap-y-2'>
      {props.byline}
      {props.labels}
    </div>
    {props.markdown && <Markdown className='relaxed' markdown={props.markdown} markedRange={props.markedRange} />}
    {!props.markdown && !props.childPages && <p className='relaxed text-sm opacity-70'>{t('pages.empty')}</p>}
    {props.learning && <div className='relaxed'>{props.learning}</div>}
    {props.attachments && <div className='relaxed'>{props.attachments}</div>}
    {props.childPages && <div className='relaxed'>{props.childPages}</div>}
  </article>
}
