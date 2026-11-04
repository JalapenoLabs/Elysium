// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'
import type {
  ProjectManagerTurn,
  ProjectManagerTurnStatus,
  ProjectManagerWake,
} from '../../api/routes/projectManagerRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { selectMe } from '../../store/authSlice'
import { useAppSelector } from '../../store/hooks'

// User interface
import { Spinner } from '@heroui/react'
import { LuChevronRight, LuCircleCheck, LuCircleX } from 'react-icons/lu'
import { ProjectManagerTurnLog } from './ProjectManagerTurnLog'
import { RunDuration } from '../Runs/RunDuration'

// Misc
import { DateText } from '../../format/DateText'
import { formatShortDateTime } from '../../format/dates'
import { useUserNames } from '../../hooks/useUserNames'
import { ELYSIA_USER_ID } from '../../constants'
import { groupWakes, turnStatusLabelKeys } from './projectManagerPresentation'

type Props = {
  projectId: string
  turn: ProjectManagerTurn
}

const statusIcons = {
  running: <Spinner size='sm' />,
  succeeded: <LuCircleCheck className='size-4 text-success' aria-hidden />,
  failed: <LuCircleX className='size-4 text-danger' aria-hidden />,
} as const satisfies Record<ProjectManagerTurnStatus, ReactNode>

// One turn of a project's Project Manager, as GitHub Actions shows a step: collapsed to its
// state, its summary (or why it failed), when it started, and how long it took. Opening it shows
// what woke it, what it answered, and its log. A running or failed turn starts open.
export function ProjectManagerTurnRow(props: Props) {
  const { t } = useTranslation('projects')
  const { turn } = props
  const [ isOpen, setIsOpen ] = useState(turn.status !== 'succeeded')
  const userNames = useUserNames()
  const me = useAppSelector(selectMe)
  const woken = groupWakes(turn.wakes)

  let line: string = t('manager.turn.noSummary')
  if (turn.status === 'running') {
    line = t('manager.turn.working')
  }
  else if (turn.status === 'failed' && turn.error) {
    line = turn.error
  }
  else if (turn.summary) {
    line = turn.summary
  }

  function author(wake: ProjectManagerWake) {
    if (wake.createdBy === ELYSIA_USER_ID) {
      return t('manager.turn.fromElysia')
    }
    if (wake.createdBy === me?.user.id) {
      return t('manager.turn.fromYou')
    }
    return userNames[wake.createdBy] ?? t('manager.turn.fromSomeone')
  }

  return <li className='compact overflow-hidden rounded-lg border border-separator'>
    <div className='level gap-3 px-3 py-2'>
      <button
        type='button'
        className='level-left min-w-0 flex-1 cursor-pointer gap-2 text-left'
        aria-expanded={isOpen}
        onClick={() => setIsOpen((open) => !open)}
      >
        <LuChevronRight
          className={`size-4 shrink-0 opacity-60 transition-transform ${isOpen ? 'rotate-90' : ''}`}
          aria-hidden
        />
        <span className='grid size-4 shrink-0 place-items-center' title={t(turnStatusLabelKeys[turn.status])}>{
          statusIcons[turn.status]
        }</span>
        <span className={`truncate text-sm ${turn.status === 'failed' ? 'text-danger' : ''}`}>{line}</span>
      </button>
      <div className='level-right shrink-0 gap-3 text-sm opacity-80'>
        <DateText value={turn.startedAt} isFocusable={false} />
        <RunDuration
          startedAt={turn.startedAt}
          finishedAt={turn.finishedAt}
          isLive={turn.status === 'running'}
        />
      </div>
    </div>

    {isOpen && <div className='border-t border-separator px-4 py-3'>
      {/* A turn cut short gave what woke it to the next one, so it may hold nothing. */}
      {turn.wakes.length > 0 && <h4 className='compact text-xs font-semibold uppercase opacity-60'>{
        t('manager.turn.wokenBy')
      }</h4>}
      <ul className='relaxed flex flex-col gap-2 text-sm empty:hidden'>
        {woken.messages.map((message) => <li key={message.id} className='rounded-lg bg-default p-3'>
          <div className='mb-1 text-xs'>
            <span className='font-semibold'>{author(message)}</span>
            <span className='ml-2 opacity-60'>{formatShortDateTime(message.createdAt)}</span>
          </div>
          <p className='whitespace-pre-line'>{message.body}</p>
        </li>)}
        {woken.records > 0 && <li className='opacity-80'>{t('manager.turn.records', { count: woken.records })}</li>}
        {woken.batches > 0 && <li className='opacity-80'>{t('manager.turn.batches', { count: woken.batches })}</li>}
      </ul>

      {turn.reply && <div className='relaxed'>
        <h4 className='compact text-xs font-semibold uppercase opacity-60'>{t('manager.turn.reply')}</h4>
        <p className='rounded-lg bg-surface p-3 text-sm whitespace-pre-line'>{turn.reply}</p>
      </div>}

      {turn.status !== 'running' && <div>
        <h4 className='compact text-xs font-semibold uppercase opacity-60'>{t('manager.turn.log')}</h4>
        <ProjectManagerTurnLog projectId={props.projectId} turnId={turn.id} />
      </div>}
    </div>}
  </li>
}
