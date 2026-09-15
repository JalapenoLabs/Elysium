// Copyright © 2026 Jalapeno Labs

import type { Project } from '../../api/routes/projectRoutes'

// Core
import { useCallback, useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { shallowEqual } from 'react-redux'
import { selectSessionCountsByProjectId } from '../../store/codingSessionsSlice'
import { useAppSelector } from '../../store/hooks'
import { selectAllProjects } from '../../store/projectsSlice'

// User interface
import { Button, Spinner, useOverlayState } from '@heroui/react'
import { LuPlus } from 'react-icons/lu'
import { DeleteProjectDialog } from './DeleteProjectDialog'
import { ProjectFormModal } from './ProjectFormModal'
import { ProjectTable } from './ProjectTable'

// Misc
import { useCodingSessionsLoader, useProjectsLoader } from '../../hooks/useServerData'

export function ProjectsPage() {
  const { t } = useTranslation('projects')
  const projects = useAppSelector(selectAllProjects)
  const status = useProjectsLoader()
  useCodingSessionsLoader()
  const sessionCounts = useAppSelector(selectSessionCountsByProjectId, shallowEqual)

  const formState = useOverlayState()
  const deleteState = useOverlayState()
  const [ selectedProject, setSelectedProject ] = useState<Project | null>(null)
  // Remounting the form per opening resets it to the chosen project's values.
  const [ formSession, setFormSession ] = useState(0)

  // Stable so the table's columns are not rebuilt on every render. The overlay state
  // object is new each render, but its `open` callbacks are memoized.
  const openFormOverlay = formState.open
  const openForm = useCallback((project: Project | null) => {
    setSelectedProject(project)
    setFormSession((session) => session + 1)
    openFormOverlay()
  }, [ openFormOverlay ])

  const openDeleteOverlay = deleteState.open
  const openDelete = useCallback((project: Project) => {
    setSelectedProject(project)
    openDeleteOverlay()
  }, [ openDeleteOverlay ])

  return <div className='container'>
    <div className='level relaxed items-start'>
      <div>
        <h1 className='text-3xl font-bold'>{
          t('title')
        }</h1>
        <p className='mt-1 max-w-2xl text-sm opacity-70'>{
          t('description')
        }</p>
      </div>
      <Button
        size='sm'
        className='shrink-0'
        onPress={() => openForm(null)}
      >
        <LuPlus className='size-4' aria-hidden />
        <span>{t('add')}</span>
      </Button>
    </div>

    {status === 'loading' && <div className='grid place-items-center py-16'>
      <Spinner />
    </div>}

    {status === 'failed' && <p className='py-10 text-center text-sm text-danger'>{
      t('table.loadError')
    }</p>}

    {status === 'loaded' && <ProjectTable
      projects={projects}
      sessionCounts={sessionCounts}
      onEdit={openForm}
      onDelete={openDelete}
    />}

    <ProjectFormModal
      key={formSession}
      state={formState}
      project={selectedProject}
    />
    <DeleteProjectDialog
      state={deleteState}
      project={selectedProject}
      sessionCount={selectedProject
        ? sessionCounts[selectedProject.id] ?? 0
        : 0}
    />
  </div>
}
