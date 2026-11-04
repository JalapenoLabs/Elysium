// Copyright © 2026 Jalapeno Labs

import type { DataTableAction } from '@jalapenolabs/uikit'
import type { Initiative } from '../../api/routes/initiativeRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectDeletedInitiatives, selectLiveInitiatives } from '../../store/initiativesSlice'

// User interface
import { Label, Spinner, Switch } from '@heroui/react'
import { LuUndo2 } from 'react-icons/lu'
import { EmptyNotice } from '../../components/EmptyNotice'
import { OptionSelect } from '../../components/OptionSelect'
import { InitiativeTable } from './InitiativeTable'

// Misc
import { INITIATIVE_STATES } from '../../api/routes/initiativeRoutes'
import { useDeletedInitiativesLoader, useInitiativesLoader } from '../../hooks/useServerData'
import { initiativeStateLabelKeys } from './initiativePresentation'
import { useInitiativeActions } from './useInitiativeActions'

// Every state, as the state filter's key.
const ALL_STATES = 'all'

// `/action-items/initiatives`: every initiative with its progress, the active ones first
// by default. Deleted ones show in their place when asked for, each with Restore.
export function InitiativesPage() {
  const { t } = useTranslation('initiatives')
  const actions = useInitiativeActions()
  const [ stateFilter, setStateFilter ] = useState<string>('active')
  const [ isShowingDeleted, setIsShowingDeleted ] = useState(false)
  const liveStatus = useInitiativesLoader()
  const deletedStatus = useDeletedInitiativesLoader(isShowingDeleted)
  const liveInitiatives = useAppSelector(selectLiveInitiatives)
  const deletedInitiatives = useAppSelector(selectDeletedInitiatives)

  const status = isShowingDeleted
    ? deletedStatus
    : liveStatus
  const shown = isShowingDeleted
    ? deletedInitiatives
    : liveInitiatives
  const initiatives = stateFilter === ALL_STATES
    ? shown
    : shown.filter((initiative) => initiative.state === stateFilter)

  // Only a deleted initiative has anything to do from its row. Restoring resolves once the answer
  // is in, so its button spins until then.
  const deletedRowActions: DataTableAction<Initiative>[] = [{
    id: 'restore',
    label: t('list.restore'),
    icon: <LuUndo2 className='size-4' aria-hidden />,
    onPress: async (row) => {
      await actions.restore(row.original)
    },
  }]

  const stateOptions = [
    { id: ALL_STATES, label: t('list.allStates') },
    ...INITIATIVE_STATES.map((state) => ({ id: state, label: t(initiativeStateLabelKeys[state]) })),
  ]

  return <div>
    <div className='relaxed flex flex-wrap items-end gap-3'>
      <OptionSelect
        className='w-48'
        label={t('list.state')}
        options={stateOptions}
        value={stateFilter}
        onChange={setStateFilter}
      />
      <Switch className='h-10 justify-center' isSelected={isShowingDeleted} onChange={setIsShowingDeleted}>
        <Switch.Content>
          <Switch.Control>
            <Switch.Thumb />
          </Switch.Control>
          <Label>{t('list.deleted')}</Label>
        </Switch.Content>
      </Switch>
    </div>

    {status === 'loading' && <div className='grid place-items-center py-16'>
      <Spinner />
    </div>}

    {status === 'failed' && <p className='py-10 text-center text-sm text-danger'>{t('list.loadError')}</p>}

    {status === 'loaded' && !initiatives.length && <EmptyNotice>{
      isShowingDeleted
        ? t('list.emptyDeleted')
        : t('list.empty')
    }</EmptyNotice>}

    {status === 'loaded' && initiatives.length > 0 && <InitiativeTable
      initiatives={initiatives}
      label={t('title')}
      storageId={isShowingDeleted
        ? 'elysium.initiatives.deleted.table.v1'
        : 'elysium.initiatives.table.v1'}
      isSearchable
      rowActions={isShowingDeleted
        ? deletedRowActions
        : undefined}
    />}
  </div>
}
