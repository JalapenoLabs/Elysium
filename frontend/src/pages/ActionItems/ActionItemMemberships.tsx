// Copyright © 2026 Jalapeno Labs

import type { ActionItem } from '../../api/routes/actionItemRoutes'

// Core
import { useTranslation } from 'react-i18next'
import { shallowEqual } from 'react-redux'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectInitiativeNamesById } from '../../store/initiativesSlice'
import { selectProjectNamesById } from '../../store/projectsSlice'

// User interface
import { Link } from '@heroui/react'
import { LuFolderKanban, LuTarget } from 'react-icons/lu'

// Misc
import { useInitiativesLoader, useProjectsLoader } from '../../hooks/useServerData'
import { getInitiativeViewUrl, getProjectViewUrl } from '../../urls'

type Props = {
  item: ActionItem
}

// The projects and initiatives an item belongs to, each a link to its page.
export function ActionItemMemberships(props: Props) {
  const { t } = useTranslation('actionItems')
  useProjectsLoader()
  useInitiativesLoader()
  const projectNames = useAppSelector(selectProjectNamesById, shallowEqual)
  const initiativeNames = useAppSelector(selectInitiativeNamesById, shallowEqual)

  const projectIds = props.item.projectIds.filter((projectId) => projectNames[projectId])
  const initiativeIds = props.item.initiativeIds.filter((initiativeId) => initiativeNames[initiativeId])

  if (!projectIds.length && !initiativeIds.length) {
    return <p className='text-sm opacity-60'>{t('memberships.none')}</p>
  }

  return <div className='flex flex-wrap gap-x-4 gap-y-1 text-sm'>
    {projectIds.map((projectId) => <Link
      key={projectId}
      href={getProjectViewUrl(projectId)}
      className='inline-flex items-center gap-1 text-link no-underline hover:underline'
    >
      <LuFolderKanban className='size-4' aria-label={t('memberships.project')} />
      <span>{projectNames[projectId]}</span>
    </Link>)}
    {initiativeIds.map((initiativeId) => <Link
      key={initiativeId}
      href={getInitiativeViewUrl(initiativeId)}
      className='inline-flex items-center gap-1 text-link no-underline hover:underline'
    >
      <LuTarget className='size-4' aria-label={t('memberships.initiative')} />
      <span>{initiativeNames[initiativeId]}</span>
    </Link>)}
  </div>
}
