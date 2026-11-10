// Copyright © 2026 Jalapeno Labs

import type { KnowledgePage } from '../../../api/routes/pageRoutes'
import type { AddPickerOption } from '../../../components/AddPicker'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectAllProjects, selectProjectEntities } from '../../../store/projectsSlice'

// User interface
import { Button, Chip, Link, Tooltip } from '@heroui/react'
import { LuFolder, LuX } from 'react-icons/lu'
import { AddPicker } from '../../../components/AddPicker'

// Misc
import { usePendingAction } from '../../../hooks/usePendingAction'
import { useProjectsLoader } from '../../../hooks/useServerData'
import { getProjectDocsUrl } from '../../../urls'
import { usePageProjectLinks } from './usePageProjectLinks'

type Props = {
  page: KnowledgePage
}

// The projects a page is linked to, each opening that project's docs, and the field that adds
// another, picked by name. A deleted page changes no further, so it only reads them.
export function PageProjects(props: Props) {
  const { page } = props
  const { t } = useTranslation('knowledge')
  useProjectsLoader()
  const projects = useAppSelector(selectAllProjects)
  const projectsById = useAppSelector(selectProjectEntities)
  const links = usePageProjectLinks()
  const pending = usePendingAction<string>()

  const isReadOnly = Boolean(page.deletedAt)
  const addable: AddPickerOption[] = []
  for (const project of projects) {
    if (!page.projectIds.includes(project.id)) {
      addable.push({ id: project.id, label: project.name })
    }
  }

  function projectName(projectId: string) {
    return projectsById[projectId]?.name ?? t('projectLinks.unknownProject')
  }

  function linkTo(projectId: string) {
    return { pageId: page.id, pageTitle: page.title, projectId, projectName: projectName(projectId) }
  }

  if (isReadOnly && !page.projectIds.length) {
    return null
  }

  return <div className='flex flex-wrap items-center gap-1.5'>
    {Boolean(page.projectIds.length) && <ul aria-label={t('projectLinks.label')} className='contents'>{
      page.projectIds.map((projectId) => {
        const name = projectName(projectId)
        const removeLabel = t('projectLinks.remove', { project: name })

        return <li key={projectId} className='flex items-center'>
          <Link href={getProjectDocsUrl(projectId)} className='rounded-full no-underline hover:opacity-80'>
            <Chip size='sm' variant='soft' color='accent'>
              <LuFolder className='size-3' aria-hidden />
              <Chip.Label>{name}</Chip.Label>
            </Chip>
          </Link>
          {/* A project's brief stays linked to it for as long as the project exists. */}
          {!isReadOnly && page.briefProjectId !== projectId && <Tooltip delay={400}>
            <Button
              isIconOnly
              size='sm'
              variant='ghost'
              className='size-5 min-w-0'
              aria-label={removeLabel}
              isPending={pending.pendingKey === projectId}
              isDisabled={pending.isOtherPending(projectId)}
              onPress={() => void pending.run(projectId, () => links.unlink(linkTo(projectId)))}
            >
              <LuX className='size-3' aria-hidden />
            </Button>
            <Tooltip.Content>{removeLabel}</Tooltip.Content>
          </Tooltip>}
        </li>
      })
    }</ul>}
    {!isReadOnly && <AddPicker
      id='page-project-add'
      className='w-44'
      label={t('projectLinks.add')}
      emptyLabel={t('projectLinks.noneToAdd')}
      options={addable}
      isDisabled={pending.isAnyPending}
      onPick={(projectId) => void pending.run(projectId, () => links.link(linkTo(projectId)))}
    />}
  </div>
}
