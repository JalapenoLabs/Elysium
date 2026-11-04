// Copyright © 2026 Jalapeno Labs

import type { KnowledgeAnswer } from '../../../api/routes/knowledgeRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Link, Tooltip } from '@heroui/react'

// Misc
import { DateText } from '../../../format/DateText'
import { getKnowledgeDocumentUrl } from '../../../urls'
import { presentKnowledgeSource } from '../knowledgePresentation'
import { presentClaims } from './knowledgeAskPresentation'

type Props = {
  answer: KnowledgeAnswer
  searchedByMeaning: boolean
}

// A citation reads as a small numbered mark after the claim it backs, and opens its passage.
const CITATION_CLASS = [
  'ml-0.5 inline-flex min-w-5 items-center justify-center rounded-md bg-default px-1',
  'align-super text-[0.7rem] font-medium text-link no-underline hover:bg-surface-secondary',
].join(' ')

// An answer: its claims read as one paragraph, each followed by the numbers of the sources it
// cites, each opening the passage it cites; then the sources those numbers name, with where each
// came from, who said it, and when. What was left out for citing nothing is counted below.
export function KnowledgeAnswerView(props: Props) {
  const { t } = useTranslation('knowledge')
  const { answer } = props
  const claims = presentClaims(answer)

  return <>
    {!claims.length && <p className='text-sm opacity-70'>{t('ask.nothingFound')}</p>}
    {claims.length > 0 && <p className='relaxed leading-relaxed'>{
      claims.map((claim, index) => <span key={index}>
        <span>{claim.text}</span>
        {claim.sources.map((source) => {
          const title = source.citation.title || presentKnowledgeSource(source.citation.source, t).label
          return <Tooltip key={source.number} delay={300}>
            <Link
              href={getKnowledgeDocumentUrl(source.citation.documentId, source.position)}
              aria-label={t('ask.citation', { number: source.number, title })}
              className={CITATION_CLASS}
            >{
                source.number
              }</Link>
            <Tooltip.Content>{title}</Tooltip.Content>
          </Tooltip>
        })}
        {' '}
      </span>)
    }</p>}

    {answer.sources.length > 0 && <section aria-label={t('ask.sources')} className='relaxed'>
      <h4 className='compact text-xs font-medium opacity-70'>{t('ask.sources')}</h4>
      <ol className='flex flex-col gap-1.5'>{
        answer.sources.map((source) => {
          const presented = presentKnowledgeSource(source.citation.source, t)
          const SourceIcon = presented.icon
          return <li key={source.number} className='flex items-start gap-2 text-sm'>
            <span className='w-5 shrink-0 text-right text-xs opacity-70'>{source.number}</span>
            <SourceIcon className={`mt-0.5 size-4 shrink-0 ${presented.className}`} aria-hidden />
            <div className='min-w-0'>
              <Link
                href={getKnowledgeDocumentUrl(source.citation.documentId, source.position)}
                className='text-link'
              >{
                  source.citation.title || presented.label
                }</Link>
              <div className='flex flex-wrap items-center gap-x-2 text-xs opacity-70'>
                <span>{presented.label}</span>
                <span aria-hidden>·</span>
                <span>{source.citation.person.name}</span>
                <span aria-hidden>·</span>
                <DateText value={source.citation.occurredAt} isFocusable={false} />
              </div>
            </div>
          </li>
        })
      }</ol>
    </section>}

    {answer.unsupportedClaims > 0 && <p className='text-xs opacity-70'>{
      t('ask.unsupported', { count: answer.unsupportedClaims })
    }</p>}
    {!props.searchedByMeaning && <p className='text-xs opacity-70'>{t('search.wordsOnly')}</p>}
  </>
}
