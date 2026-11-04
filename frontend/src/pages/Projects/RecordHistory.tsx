// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// User interface
import { Button, Spinner, Tooltip } from '@heroui/react'
import { LuUndo2 } from 'react-icons/lu'

// Misc
import { listRecordHistory, recordHistoryKey } from '../../api/routes/timelineRoutes'
import { DateText } from '../../format/DateText'
import { formatPercent } from '../../format/numbers'
import { usePendingAction } from '../../hooks/usePendingAction'
import { boundedConfidence, describeAttributionChange } from './timelinePresentation'
import { useUndoRecordChange } from './useUndoRecordChange'

type Props = {
  recordId: string
  // A project's name, or what a removed one reads as.
  projectName: (projectId: string) => string
}

// Every change to what a record is about, newest first: Elysia's as segmenting read it, and the
// person's own, each with Undo. Its own read, since no other view shows it; a change the event
// stream announces reloads it.
export function RecordHistory(props: Props) {
  const { t } = useTranslation([ 'timeline', 'common' ])
  const undo = useUndoRecordChange()
  const pending = usePendingAction<string>()
  const { data, error } = useSWR(recordHistoryKey(props.recordId), () => listRecordHistory(props.recordId))

  if (!data) {
    if (error) {
      console.debug('RecordHistory could not load a history', { error, recordId: props.recordId })
      return <p className='text-xs text-danger'>{t('history.loadError')}</p>
    }
    return <div className='grid place-items-center py-3'><Spinner size='sm' /></div>
  }
  if (!data.history.length) {
    return <p className='text-xs opacity-60'>{t('history.empty')}</p>
  }

  return <ol aria-label={t('history.label')} className='flex flex-col gap-1.5 border-l border-separator pl-3'>{
    data.history.toReversed().map((entry) => {
      const change = describeAttributionChange(entry)
      const percent = change.confidence === null
        ? ''
        : formatPercent(boundedConfidence(change.confidence))
      return <li key={entry.id} className='level items-center gap-2 text-xs'>
        <span>
          <span>{t(change.key, { project: props.projectName(entry.projectId), percent })}</span>
          <DateText value={entry.createdAt} className='ml-2 opacity-50' />
        </span>
        <Tooltip delay={300}>
          <Button
            isIconOnly
            size='sm'
            variant='ghost'
            aria-label={t('history.undo')}
            isPending={pending.pendingKey === entry.id}
            isDisabled={pending.isOtherPending(entry.id)}
            onPress={() => void pending.run(entry.id, () => undo(props.recordId, entry.id))}
          >
            <LuUndo2 className='size-3.5' aria-hidden />
          </Button>
          <Tooltip.Content>
            <span>{t('history.undo')}</span>
          </Tooltip.Content>
        </Tooltip>
      </li>
    })
  }</ol>
}
