// Copyright © 2026 Jalapeno Labs

import type { KnowledgeResult } from '../../api/routes/knowledgeRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Card, Link } from '@heroui/react'

// Misc
import { DateText } from '../../format/DateText'
import { listFormat } from '../../format/intl'
import { getKnowledgeDocumentUrl } from '../../urls'
import { presentKnowledgeSource, withoutTitle } from './knowledgePresentation'

type Props = {
  result: KnowledgeResult
}

// A passage reads as a quote of its document, in the page's text color rather than a link's,
// and fills under the mouse to show it opens.
const PASSAGE_LINK_CLASS = [
  'block rounded-lg border-l-2 border-separator py-1 pl-3',
  'text-sm text-foreground no-underline hover:bg-default',
].join(' ')

// One document a search found: where it came from, whose record it is and when it was said, and
// its best passages, each a link that opens the document at that passage. A passage runs on as
// one paragraph cut to three lines, so the lines that matched show; the document shows it whole.
export function KnowledgeResultCard(props: Props) {
  const { t } = useTranslation('knowledge')
  const { citation, passages, alsoRecordedBy } = props.result
  const source = presentKnowledgeSource(citation.source, t)
  const SourceIcon = source.icon
  const bestPosition = passages[0]?.position

  return <Card className='relaxed p-4'>
    <div className='flex items-start gap-3'>
      <SourceIcon className={`mt-0.5 size-5 shrink-0 ${source.className}`} aria-hidden />
      <div className='min-w-0 flex-1'>
        <Link
          href={getKnowledgeDocumentUrl(citation.documentId, bestPosition)}
          className='font-medium text-link'
        >{
            citation.title || source.label
          }</Link>
        <div className='flex flex-wrap items-center gap-x-2 text-xs opacity-70'>
          <span>{source.label}</span>
          <span aria-hidden>·</span>
          <span>{citation.person.name}</span>
          <span aria-hidden>·</span>
          <DateText value={citation.occurredAt} isFocusable={false} />
        </div>
      </div>
    </div>
    <ul className='flex flex-col gap-2'>{
      passages.map((passage) => <li key={passage.position}>
        <Link
          href={getKnowledgeDocumentUrl(citation.documentId, passage.position)}
          className={PASSAGE_LINK_CLASS}
        >
          <span className='line-clamp-3'>{withoutTitle(passage, citation.title)}</span>
        </Link>
      </li>)
    }</ul>
    {alsoRecordedBy.length > 0 && <p className='mt-3 text-xs opacity-70'>{
      t('result.alsoRecordedBy', {
        names: listFormat({ type: 'conjunction' }).format(alsoRecordedBy.map((copy) => copy.person.name)),
      })
    }</p>}
  </Card>
}
