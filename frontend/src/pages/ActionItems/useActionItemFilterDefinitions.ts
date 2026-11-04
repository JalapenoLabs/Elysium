// Copyright © 2026 Jalapeno Labs

import type { SmartTableFilter } from '@jalapenolabs/uikit'
import type { ActionItem } from '../../api/routes/actionItemRoutes'

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectLiveInitiatives } from '../../store/initiativesSlice'
import { selectAllProjects } from '../../store/projectsSlice'

// Misc
import { ACTION_ITEM_STATES } from '../../api/routes/actionItemRoutes'
import { useInitiativesLoader, useProjectsLoader } from '../../hooks/useServerData'
import { ACTION_ITEM_FILTER_IDS, NO_PROJECT } from './actionItemFilters'
import { stateLabelKeys } from './actionItemPresentation'

// The All items filters as the table's drawer offers them: states, project (or no project),
// initiative, waiting, snoozed, and deleted items instead of the rest. None carries `matches`:
// the page applies them itself (`filterActionItems`), so it knows when nothing matches.
export function useActionItemFilterDefinitions(): SmartTableFilter<ActionItem>[] {
  const { t } = useTranslation('actionItems')
  useProjectsLoader()
  useInitiativesLoader()
  const projects = useAppSelector(selectAllProjects)
  const initiatives = useAppSelector(selectLiveInitiatives)

  return useMemo(() => [
    {
      id: ACTION_ITEM_FILTER_IDS.states,
      label: t('filters.state'),
      kind: 'multiSelect',
      options: ACTION_ITEM_STATES.map((state) => ({ value: state, label: t(stateLabelKeys[state]) })),
    },
    {
      id: ACTION_ITEM_FILTER_IDS.project,
      label: t('filters.project'),
      kind: 'select',
      options: [
        { value: NO_PROJECT, label: t('filters.noProject') },
        ...projects.map((project) => ({ value: project.id, label: project.name })),
      ],
    },
    {
      id: ACTION_ITEM_FILTER_IDS.initiative,
      label: t('filters.initiative'),
      kind: 'select',
      options: initiatives.map((initiative) => ({ value: initiative.id, label: initiative.name })),
    },
    { id: ACTION_ITEM_FILTER_IDS.waiting, label: t('filters.waiting'), kind: 'boolean' },
    { id: ACTION_ITEM_FILTER_IDS.snoozed, label: t('filters.snoozed'), kind: 'boolean' },
    { id: ACTION_ITEM_FILTER_IDS.deleted, label: t('filters.deleted'), kind: 'boolean' },
  ], [ t, projects, initiatives ])
}
