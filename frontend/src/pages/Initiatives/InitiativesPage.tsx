// Copyright © 2026 Jalapeno Labs

import type { DataTableAction, SmartTableFilter } from '@jalapenolabs/uikit'
import type { Initiative } from '../../api/routes/initiativeRoutes'

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { useSearchParams } from 'react-router'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectDeletedInitiatives, selectLiveInitiatives } from '../../store/initiativesSlice'
import { selectAllProjects } from '../../store/projectsSlice'

// User interface
import { Spinner } from '@heroui/react'
import { LuUndo2 } from 'react-icons/lu'
import { EmptyNotice } from '../../components/EmptyNotice'
import { InitiativeTable } from './InitiativeTable'

// Misc
import { INITIATIVE_STATES } from '../../api/routes/initiativeRoutes'
import { useDeletedInitiativesLoader, useInitiativesLoader, useProjectsLoader } from '../../hooks/useServerData'
import { usePageToolsContainer } from '../../layout/page/pageTools'
import { NO_PROJECT } from '../ActionItems/actionItemFilters'
import {
  filterInitiatives,
  fromInitiativeFilterValues,
  INITIATIVE_FILTER_IDS,
  readInitiativeFilters,
  toInitiativeFilterValues,
  writeInitiativeFilters,
} from './initiativeFilters'
import { initiativeStateLabelKeys } from './initiativePresentation'
import { useInitiativeActions } from './useInitiativeActions'
import { useInitiativeBulkActions } from './useInitiativeBulkActions'

// `/action-items/initiatives`: every initiative with its progress, its search, filters,
// Columns, and export in the titlebar, the active ones by default. Filters live in the address,
// like the item list's. Deleted ones show in their place when asked for, each with Restore.
// Selecting rows offers a state change and delete, or restore, on the floating bar.
export function InitiativesPage() {
  const { t } = useTranslation('initiatives')
  const actions = useInitiativeActions()
  const bulkActions = useInitiativeBulkActions()
  const toolsContainer = usePageToolsContainer()
  const [ searchParams, setSearchParams ] = useSearchParams()
  const filters = readInitiativeFilters(searchParams)
  const liveStatus = useInitiativesLoader()
  const deletedStatus = useDeletedInitiativesLoader(filters.deleted)
  useProjectsLoader()
  const liveInitiatives = useAppSelector(selectLiveInitiatives)
  const deletedInitiatives = useAppSelector(selectDeletedInitiatives)
  const projects = useAppSelector(selectAllProjects)

  const status = filters.deleted
    ? deletedStatus
    : liveStatus
  const initiatives = filterInitiatives(
    filters.deleted
      ? deletedInitiatives
      : liveInitiatives,
    filters,
  )

  // None carries `matches`: the page applies them itself, so it knows when nothing matches.
  const filterDefinitions = useMemo((): SmartTableFilter<Initiative>[] => [
    {
      id: INITIATIVE_FILTER_IDS.states,
      label: t('list.state'),
      kind: 'multiSelect',
      options: INITIATIVE_STATES.map((state) => ({ value: state, label: t(initiativeStateLabelKeys[state]) })),
    },
    {
      id: INITIATIVE_FILTER_IDS.project,
      label: t('list.project'),
      kind: 'select',
      options: [
        { value: NO_PROJECT, label: t('list.noProject') },
        ...projects.map((project) => ({ value: project.id, label: project.name })),
      ],
    },
    { id: INITIATIVE_FILTER_IDS.deleted, label: t('list.deleted'), kind: 'boolean' },
  ], [ t, projects ])

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

  return <div>
    {status === 'loading' && <div className='grid place-items-center py-16'>
      <Spinner />
    </div>}

    {status === 'failed' && <p className='py-10 text-center text-sm text-danger'>{t('list.loadError')}</p>}

    {/* Rendered with no rows too, so the titlebar keeps the filters that emptied it. */}
    {status === 'loaded' && <InitiativeTable
      // The live and deleted lists keep their own layouts, so switching starts the other fresh.
      key={filters.deleted
        ? 'deleted'
        : 'live'}
      initiatives={initiatives}
      label={t('title')}
      storageId={filters.deleted
        ? 'elysium.initiatives.deleted.table.v1'
        : 'elysium.initiatives.table.v1'}
      controlsContainer={toolsContainer}
      filters={{
        definitions: filterDefinitions,
        values: toInitiativeFilterValues(filters),
        onValuesChange: (values) => setSearchParams(
          writeInitiativeFilters(fromInitiativeFilterValues(values)),
          { replace: true },
        ),
      }}
      emptyNotice={<EmptyNotice>{
        filters.deleted
          ? t('list.emptyDeleted')
          : t('list.empty')
      }</EmptyNotice>}
      rowActions={filters.deleted
        ? deletedRowActions
        : undefined}
      selectionActions={filters.deleted
        ? bulkActions.deletedActions
        : bulkActions.liveActions}
    />}
  </div>
}
