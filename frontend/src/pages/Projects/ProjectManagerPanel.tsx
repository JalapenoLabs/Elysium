// Copyright © 2026 Jalapeno Labs

import type { LoadStatus } from '../../hooks/useServerData'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectProjectManager } from '../../store/projectManagersSlice'

// User interface
import { Chip, Spinner } from '@heroui/react'
import { ConfigurationHoldAlert } from '../../components/ConfigurationHoldAlert'
import { ProjectManagerComposer } from './ProjectManagerComposer'
import { ProjectManagerTurnRow } from './ProjectManagerTurnRow'
import { ProjectManagerWrites } from './ProjectManagerWrites'

// Misc
import { formatDateTime } from '../../format/dates'
import { managerStatusChipColors, managerStatusLabelKeys } from './projectManagerPresentation'

type Props = {
  projectId: string
  // How its loader stands; `ProjectLists` loads it for the tabs' counts.
  status: LoadStatus
}

// The project's Project Manager: what it is doing, why its waiting work waits on the
// configuration repository, how its changes reach the work, a box to write to it, and its latest
// turns, newest first, each collapsed to its summary.
export function ProjectManagerPanel(props: Props) {
  const { t } = useTranslation('projects')
  const manager = useAppSelector((state) => selectProjectManager(state, props.projectId))

  if (!manager) {
    return <div className='grid place-items-center py-10'>{
      props.status === 'failed'
        ? <p className='text-sm text-danger'>{t('manager.loadError')}</p>
        : <Spinner />
    }</div>
  }

  return <section>
    <div className='relaxed flex flex-wrap items-center gap-3'>
      <Chip size='sm' variant='soft' color={managerStatusChipColors[manager.status]}>{
        t(managerStatusLabelKeys[manager.status])
      }</Chip>
      {manager.waiting > 0 && <span className='text-sm opacity-70'>{
        t('manager.waiting', { count: manager.waiting })
      }</span>}
      {manager.usagePausedUntil && <span className='text-sm opacity-70'>{
        t('manager.heldUntil', { time: formatDateTime(manager.usagePausedUntil) })
      }</span>}
    </div>

    {manager.configurationHold && manager.waiting > 0 && <ConfigurationHoldAlert
      hold={manager.configurationHold}
    />}

    <ProjectManagerWrites manager={manager} />

    <ProjectManagerComposer projectId={props.projectId} />

    {manager.turns.length
      ? <ol>{
        manager.turns.map((turn) => <ProjectManagerTurnRow
          key={turn.id}
          projectId={props.projectId}
          turn={turn}
        />)
      }</ol>
      : <p className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'>{
        t('manager.empty')
      }</p>}
  </section>
}
