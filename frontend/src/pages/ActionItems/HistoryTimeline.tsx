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
import { Spinner } from '@heroui/react'

// Misc
import { useInitiativesLoader, useProjectsLoader } from '../../hooks/useServerData'
import { describeActor } from './actionItemPresentation'
import { describeHistoryEntry } from './historyPresentation'

type Props = {
  entries: HistoryEntry[]
  subject: HistoryContext['subject']
  status: 'loading' | 'loaded' | 'failed'
}

// An item's or initiative's history, newest first: who did what, and when, with each field
// an edit changed.
export function HistoryTimeline(props: Props) {
  const { t, i18n } = useTranslation([ 'actionItems', 'initiatives' ])
  useProjectsLoader()
  useInitiativesLoader()
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

  const dateFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium', timeStyle: 'short' })
  const context: HistoryContext = {
    subject: props.subject,
    projectNames,
    initiativeNames,
    itemTitles,
    formatInstant: (instant) => dateFormatter.format(new Date(instant)),
  }

  return <ol className='flex flex-col gap-4 border-l border-separator pl-4'>{
    props.entries.toReversed().map((entry) => {
      const actor = describeActor(entry.actor)
      const description = describeHistoryEntry(entry, context, t)
      return <li key={entry.id} className='text-sm'>
        <p>
          <span className='font-semibold'>{t(actor.key, actor.values)}</span>
          <span className='ml-1'>{description.summary}</span>
        </p>
        {description.details.map((detail, index) => <p
          key={index}
          className='mt-1 line-clamp-3 text-xs whitespace-pre-line opacity-70'
        >{detail}</p>)}
        <p className='mt-1 text-xs opacity-50'>{context.formatInstant(entry.createdAt)}</p>
      </li>
    })
  }</ol>
}
