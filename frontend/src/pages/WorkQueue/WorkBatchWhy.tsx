// Copyright © 2026 Jalapeno Labs

import type { WorkBatchWhy as Why } from '../../api/routes/workQueueRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Link } from '@heroui/react'

// Misc
import { formatDateTime } from '../../format/dates'
import { getActionItemViewUrl, getKnowledgeDocumentUrl } from '../../urls'
import { sourceLabelKey } from '../Projects/timelinePresentation'

type Props = {
  why: Why
}

// Why a batch exists, as its Project Manager named it: the records that called for it, each
// opening in the knowledge base, where any member reads what was ingested, with where it came from
// and when; the messages sent to its Project Manager that did, with who wrote each and when; and
// the action items it works on, each opening its item.
export function WorkBatchWhy(props: Props) {
  const { t } = useTranslation([ 'workQueue', 'timeline' ])
  const { records, messages, actionItems } = props.why

  if (!records.length && !messages.length && !actionItems.length) {
    return <p className='text-sm opacity-70'>{t('batch.why.none')}</p>
  }

  return <ul className='flex flex-col gap-2 text-sm'>
    {messages.map((message) => <li key={message.id} className='flex flex-col'>
      <span className='line-clamp-3 whitespace-pre-wrap'>{message.body}</span>
      <span className='text-xs opacity-70'>{
        t('batch.why.messageFrom', {
          author: message.author,
          time: formatDateTime(message.sentAt),
        })
      }</span>
    </li>)}
    {records.map((record) => <li key={record.id} className='flex flex-col'>
      {record.documentId
        ? <Link href={getKnowledgeDocumentUrl(record.documentId)} className='text-link'>{record.title}</Link>
        : <span>{record.title}</span>}
      <span className='text-xs opacity-70'>{
        t('batch.why.recordFrom', {
          source: t(`timeline:${sourceLabelKey(record.sourceKind)}`),
          time: formatDateTime(record.occurredAt),
        })
      }</span>
    </li>)}
    {actionItems.map((item) => <li key={item.id} className='flex flex-col'>
      <Link href={getActionItemViewUrl(item.id)} className='text-link'>{item.title}</Link>
      <span className='text-xs opacity-70'>{t('batch.why.actionItem')}</span>
    </li>)}
  </ul>
}
