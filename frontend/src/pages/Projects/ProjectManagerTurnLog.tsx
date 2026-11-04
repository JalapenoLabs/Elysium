// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectTurnLog } from '../../store/projectManagersSlice'

// User interface
import { Spinner } from '@heroui/react'

// Misc
import { useProjectManagerTurnLoader } from '../../hooks/useServerData'

type Props = {
  projectId: string
  turnId: string
}

// What a turn did, numbered line by line as a step's log is: what it said and which tools it
// called. It is read when the turn is opened, and again whenever its Project Manager changes.
export function ProjectManagerTurnLog(props: Props) {
  const { t } = useTranslation('projects')
  const status = useProjectManagerTurnLoader(props.projectId, props.turnId, true)
  const log = useAppSelector((state) => selectTurnLog(state, props.turnId))

  if (log === undefined) {
    return status === 'failed'
      ? <p className='text-sm text-danger'>{t('manager.turn.logError')}</p>
      : <Spinner size='sm' />
  }
  const lines = log.split('\n')
  if (lines.at(-1) === '') {
    lines.pop()
  }
  if (!lines.length) {
    return <p className='text-sm opacity-70'>{t('manager.turn.noLog')}</p>
  }
  return <ol className='max-h-96 overflow-auto rounded-lg bg-surface-secondary py-2 font-mono text-xs'>{
    lines.map((line, index) => <li
      // A log is written once, when its turn ends, so a line's place serves as its key.
      key={index}
      className='grid grid-cols-[3.5rem_1fr] gap-3 px-2'
    >
      <span className='select-none text-right opacity-50'>{index + 1}</span>
      <span className='whitespace-pre-wrap break-words'>{line}</span>
    </li>)
  }</ol>
}
