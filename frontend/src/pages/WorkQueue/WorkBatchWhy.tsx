// Copyright © 2026 Jalapeno Labs

import type { WorkBatchLink, WorkBatchWhy as Why } from '../../api/routes/workQueueRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { ChainLinkTitle } from './ChainLinkTitle'

// Misc
import { formatDateTime } from '../../format/dates'
import { sourceLabelKey } from '../Projects/timelinePresentation'
import { chainLinkIcons, chainLinkKey, chainLinkLabelKeys, isKnownChainLink } from './chainPresentation'

type Props = {
  why: Why
  // The batch whose page it is, which its chain names as itself rather than linking to.
  batchId: string
  projectId: string
}

// Why a batch exists, as one chain read top to bottom: what asked for it (a finding a person
// accepted, a message to its Project Manager, a meeting's quote, a mail, an item's issue), the
// items it works on, the batch, and the pull requests its worker opened. Each link opens where it
// lives: a record its knowledge base document, an issue its tracker, an item and a batch their
// pages, a pull request GitHub.
export function WorkBatchWhy(props: Props) {
  const { t } = useTranslation([ 'workQueue', 'timeline' ])
  const chain = props.why.chain.filter(isKnownChainLink)
  const namesNothing = chain.every((link) => link.kind === 'batch' || link.kind === 'pullRequest')

  return <>
    {namesNothing && <p className='compact text-sm opacity-70'>{t('batch.why.none')}</p>}
    <ol className='flex flex-col gap-3 border-l border-separator pl-4 text-sm'>{
      chain.map((link) => {
        const Icon = chainLinkIcons[link.kind]
        const isThisBatch = link.kind === 'batch' && link.id === props.batchId
        const label = link.kind === 'record'
          ? t(`timeline:${sourceLabelKey(link.sourceKind)}`)
          : t(chainLinkLabelKeys[link.kind])
        return <li key={chainLinkKey(link)} className='flex flex-col'>
          <span className='level justify-start gap-1 text-xs opacity-70'>
            <Icon className='size-3.5' aria-hidden />
            <span>{label}</span>
          </span>
          {isThisBatch
            ? <span>{t('batch.why.thisBatch', { title: link.title })}</span>
            : <ChainLinkTitle link={link} projectId={props.projectId} />}
          <ChainLinkDetail link={link} />
        </li>
      })
    }</ol>
  </>
}

type DetailProps = {
  link: WorkBatchLink
}

// What a link says beneath its title: a record's quote with when it was said, who sent a
// message and when, and who accepted a finding and when.
function ChainLinkDetail(props: DetailProps) {
  const { t } = useTranslation('workQueue')
  const link = props.link
  if (link.kind === 'record') {
    return <>
      {link.excerpt && <blockquote
        className='mt-1 line-clamp-3 border-l-2 border-separator pl-3 whitespace-pre-wrap opacity-80'
      >{
        link.excerpt
      }</blockquote>}
      <span className='text-xs opacity-70'>{formatDateTime(link.occurredAt)}</span>
    </>
  }
  if (link.kind === 'message') {
    return <span className='text-xs opacity-70'>{
      t('batch.why.messageFrom', { author: link.author, time: formatDateTime(link.sentAt) })
    }</span>
  }
  if (link.kind === 'finding') {
    return <span className='text-xs opacity-70'>{
      t('batch.why.acceptedBy', { person: link.acceptedBy, time: formatDateTime(link.acceptedAt) })
    }</span>
  }
  return null
}
