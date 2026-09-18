// Copyright © 2026 Jalapeno Labs

import type { Initiative } from '../../api/routes/initiativeRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectDeletedInitiatives, selectLiveInitiatives } from '../../store/initiativesSlice'

// User interface
import { Label, Spinner, Switch } from '@heroui/react'
import { EmptyNotice } from '../../components/EmptyNotice'
import { OptionSelect } from '../../components/OptionSelect'
import { InitiativeTable } from './InitiativeTable'
import { RestoreInitiativeButton } from './RestoreInitiativeButton'

// Misc
import { INITIATIVE_STATES } from '../../api/routes/initiativeRoutes'
import { useDeletedInitiativesLoader, useInitiativesLoader } from '../../hooks/useServerData'
import { initiativeStateLabelKeys } from './initiativePresentation'

// Every state, as the state filter's key.
const ALL_STATES = 'all'

// Defined once, outside the page, so the table's columns are not rebuilt on every render.
function renderRestoreButton(initiative: Initiative) {
  return <RestoreInitiativeButton initiative={initiative} />
}

// `/action-items/initiatives`: every initiative with its progress, the active ones first
// by default. Deleted ones show in their place when asked for, each with Restore.
export function InitiativesPage() {
  const { t } = useTranslation('initiatives')
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
      <Switch className='mb-2' isSelected={isShowingDeleted} onChange={setIsShowingDeleted}>
        <Switch.Control>
          <Switch.Thumb />
        </Switch.Control>
        <Switch.Content>
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
      renderRowActions={isShowingDeleted
        ? renderRestoreButton
        : undefined}
    />}
  </div>
}
