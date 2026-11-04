// Copyright © 2026 Jalapeno Labs

import type { RunHistoryEntry, RunStep } from '../../api/routes/runRoutes'

// Core
import { useTranslation } from 'react-i18next'
import { shallowEqual } from 'react-redux'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectUserNamesById } from '../../store/usersSlice'

// Misc
import { formatShortDateTime } from '../../format/dates'
import { useUsersLoader } from '../../hooks/useServerData'
import { nameFromIdentifier } from './runPresentation'

type Props = {
  history: RunHistoryEntry[]
  steps: RunStep[]
}

const kindKeys = {
  started: 'historyKinds.started',
  retried: 'historyKinds.retried',
  skipped: 'historyKinds.skipped',
  resumed: 'historyKinds.resumed',
} as const satisfies Record<RunHistoryEntry['kind'], string>

// Who started the run, and every step a person retried, skipped, or resumed, oldest first.
export function RunHistory(props: Props) {
  const { t } = useTranslation([ 'runs', 'common' ])
  useUsersLoader()
  const userNames = useAppSelector(selectUserNamesById, shallowEqual)
  const stepNames = new Map(props.steps.map((step) => [ step.id, nameFromIdentifier(step.step) ]))

  return <ol className='flex flex-col gap-1 text-sm'>{
    props.history.map((entry) => {
      const name = userNames[entry.createdBy] ?? t('common:table.someone')
      // An entry a later version wrote reads as the plainest kind rather than not at all.
      const key = kindKeys[entry.kind] ?? kindKeys.started
      const step = entry.stepJobId
        ? stepNames.get(entry.stepJobId) ?? ''
        : ''
      return <li key={entry.id} className='level-left gap-3'>
        <span className='opacity-60 tabular-nums'>{
          formatShortDateTime(entry.createdAt)
        }</span>
        <span>{t(key, { name, step })}</span>
      </li>
    })
  }</ol>
}
