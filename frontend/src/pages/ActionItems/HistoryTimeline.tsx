// Copyright © 2026 Jalapeno Labs

import type { HistoryEntry } from '../../api/routes/actionItemRoutes'
import type { HistoryContext } from './historyPresentation'

// Core
import { useTranslation } from 'react-i18next'
import { shallowEqual } from 'react-redux'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectActionItemTitlesById } from '../../store/actionItemsSlice'
import { selectInitiativeNamesById } from '../../store/initiativesSlice'
import { selectProjectNamesById } from '../../store/projectsSlice'

// User interface
import { Button, Link, Spinner, Tooltip } from '@heroui/react'
import { LuUndo2 } from 'react-icons/lu'

// Misc
import { UNDOABLE_HISTORY_KINDS } from '../../api/routes/actionItemRoutes'
import { formatDateTime } from '../../format/dates'
import { useActorNames } from '../../hooks/useActorNames'
import { usePendingAction } from '../../hooks/usePendingAction'
import { useInitiativesLoader, useProjectsLoader } from '../../hooks/useServerData'
import { getChangesetViewUrl } from '../../urls'
import { describeActor } from './actionItemPresentation'
import { describeHistoryEntry } from './historyPresentation'

type Props = {
  entries: HistoryEntry[]
  subject: HistoryContext['subject']
  status: 'loading' | 'loaded' | 'failed'
  // Undoes a change to what an item is attributed to; without it, nothing offers Undo.
  onUndo?: (entry: HistoryEntry) => Promise<void>
}

// An item's or initiative's history, newest first: who did what, and when, with each field
// an edit changed. An entry a changeset's applying or undoing recorded links to its review, and
// a change to an item's projects or its lead has Undo.
export function HistoryTimeline(props: Props) {
  const { t } = useTranslation([ 'actionItems', 'initiatives' ])
  const pending = usePendingAction<string>()
  useProjectsLoader()
  useInitiativesLoader()
  const actorNames = useActorNames()
  const projectNames = useAppSelector(selectProjectNamesById, shallowEqual)
  const initiativeNames = useAppSelector(selectInitiativeNamesById, shallowEqual)
  const itemTitles = useAppSelector(selectActionItemTitlesById, shallowEqual)

  if (props.status === 'loading') {
    return <div className='grid place-items-center py-6'>
      <Spinner size='sm' />
    </div>
  }
  if (props.status === 'failed') {
    return <p className='text-sm text-danger'>{t('history.loadError')}</p>
  }

  const context: HistoryContext = {
    subject: props.subject,
    projectNames,
    initiativeNames,
    itemTitles,
    formatInstant: (instant) => formatDateTime(instant),
  }

  return <ol className='flex flex-col gap-4 border-l border-separator pl-4'>{
    props.entries.toReversed().map((entry) => {
      const actor = describeActor(entry.actor, actorNames)
      const description = describeHistoryEntry(entry, context, t)
      const onUndo = props.onUndo
      const isUndoable = UNDOABLE_HISTORY_KINDS.some((kind) => kind === entry.kind)
      return <li key={entry.id} className='text-sm'>
        <div className='level items-start gap-2'>
          <p>
            <span className='font-semibold'>{t(actor.key, actor.values)}</span>
            <span className='ml-1'>{description.summary}</span>
          </p>
          {onUndo && isUndoable && <Tooltip delay={300}>
            <Button
              isIconOnly
              size='sm'
              variant='ghost'
              // Overlaps the row's spacing, so an entry with Undo is as tall as one without.
              className='-my-1.5 shrink-0'
              aria-label={t('history.undo')}
              isPending={pending.pendingKey === entry.id}
              isDisabled={pending.isOtherPending(entry.id)}
              onPress={() => void pending.run(entry.id, () => onUndo(entry))}
            >
              <LuUndo2 className='size-3.5' aria-hidden />
            </Button>
            <Tooltip.Content>
              <span>{t('history.undo')}</span>
            </Tooltip.Content>
          </Tooltip>}
        </div>
        {description.details.map((detail, index) => <p
          key={index}
          className='mt-1 line-clamp-3 text-xs whitespace-pre-line opacity-70'
        >{detail}</p>)}
        <p className='mt-1 text-xs opacity-50'>
          <span>{context.formatInstant(entry.createdAt)}</span>
          {entry.changesetId && <Link
            href={getChangesetViewUrl(entry.changesetId)}
            className='ml-2 text-xs text-link'
          >{
            t('history.fromChangeset')
          }</Link>}
        </p>
      </li>
    })
  }</ol>
}
