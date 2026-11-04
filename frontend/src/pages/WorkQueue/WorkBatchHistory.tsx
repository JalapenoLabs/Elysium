// Copyright © 2026 Jalapeno Labs

import type { WorkBatchPriority } from '../../api/routes/projectManagerRoutes'
import type { WorkBatchHistoryEntry } from '../../api/routes/workQueueRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Misc
import { formatShortDateTime } from '../../format/dates'
import { useUserNames } from '../../hooks/useUserNames'
import { batchPriorityLabelKeys } from '../Projects/projectManagerPresentation'
import { describeHistoryEntry } from './workQueuePresentation'

type Props = {
  history: WorkBatchHistoryEntry[]
}

// Everything that happened to a batch, oldest first: who submitted it, every change a person,
// Elysia, or its Project Manager made while it waited, and each move the pool and its worker
// made, with the words that came with it beneath its line: why it was cancelled, what its worker
// waits for, or the answer its Project Manager gave.
export function WorkBatchHistory(props: Props) {
  const { t } = useTranslation([ 'workQueue', 'common' ])
  const projectsTranslation = useTranslation('projects')
  const userNames = useUserNames()

  // A priority as a line says it, with a scheduled one's time.
  function priorityWords(priority: { priority: WorkBatchPriority, scheduledFor: string | null } | null) {
    if (!priority) {
      return ''
    }
    const label = projectsTranslation.t(batchPriorityLabelKeys[priority.priority])
    if (!priority.scheduledFor) {
      return label
    }
    return t('history.scheduledFor', { priority: label, time: formatShortDateTime(priority.scheduledFor) })
  }

  return <ol className='flex flex-col gap-2 text-sm'>{
    props.history.map((entry) => {
      const described = describeHistoryEntry(entry)
      const name = userNames[entry.createdBy] ?? t('common:table.someone')
      let line = t('history.unknown', { kind: entry.kind })
      if (described.labelKey) {
        line = t(described.labelKey, {
          name,
          before: priorityWords(described.before),
          after: priorityWords(described.after),
          time: described.until
            ? formatShortDateTime(described.until)
            : '',
        })
      }
      if (entry.kind === 'finished' && described.succeeded === false) {
        line = t('history.failed')
      }
      return <li key={entry.id} className='flex gap-3'>
        <span className='shrink-0 opacity-60 tabular-nums'>{
          formatShortDateTime(entry.createdAt)
        }</span>
        <div className='min-w-0'>
          <span>{line}</span>
          {described.note && <p className='whitespace-pre-wrap opacity-80'>{described.note}</p>}
        </div>
      </li>
    })
  }</ol>
}
