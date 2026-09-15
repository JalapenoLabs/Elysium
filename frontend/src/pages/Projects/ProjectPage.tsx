// Copyright © 2026 Jalapeno Labs

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router'

// Redux
import { selectAllCodingSessions } from '../../store/codingSessionsSlice'
import { useAppSelector } from '../../store/hooks'
import { selectProjectById } from '../../store/projectsSlice'

// User interface
import { Breadcrumbs, Link, Spinner, useOverlayState } from '@heroui/react'
import { ImagePreview } from '../../components/ImagePreview'
import { EditProjectModal } from './EditProjectModal'
import { ProjectActions } from './ProjectActions'
import { ProjectCover } from './ProjectCover'
import { ProjectSessionsTable } from './ProjectSessionsTable'

// Misc
import { getProjectCoverUrl } from '../../api/routes/projectRoutes'
import { useCodingSessionsLoader, useProjectsLoader } from '../../hooks/useServerData'
import { UrlTree } from '../../urls'

// `/projects/:projectId`: one project, its coding sessions, and what can be done to it.
export function ProjectPage() {
  const { t } = useTranslation('projects')
  const { projectId = '' } = useParams()
  const projectsStatus = useProjectsLoader()
  const sessionsStatus = useCodingSessionsLoader()
  const project = useAppSelector((state) => selectProjectById(state, projectId))
  const allSessions = useAppSelector(selectAllCodingSessions)

  const editState = useOverlayState()
  // Remounting the form per opening resets it to the project's current values.
  const [ formSession, setFormSession ] = useState(0)

  const sessions = useMemo(
    () => allSessions.filter((session) => session.projectId === projectId),
    [ allSessions, projectId ],
  )

  if (!project) {
    return <div className='container'>{
      projectsStatus === 'loading'
        ? <div className='grid place-items-center py-16'>
          <Spinner />
        </div>
        : <div className='py-16 text-center'>
          <p className='compact text-sm opacity-70'>{t('page.notFound')}</p>
          <Link href={UrlTree.projects}>{t('page.backToProjects')}</Link>
        </div>
    }</div>
  }

  const coverUrl = getProjectCoverUrl(project)

  return <div className='container'>
    <Breadcrumbs className='compact'>
      <Breadcrumbs.Item href={UrlTree.projects}>{t('title')}</Breadcrumbs.Item>
      <Breadcrumbs.Item>{project.name}</Breadcrumbs.Item>
    </Breadcrumbs>

    <div className='level relaxed items-start gap-4'>
      <div className='flex min-w-0 items-center gap-4'>
        {coverUrl && <ImagePreview
          src={coverUrl}
          alt={t('tile.coverAlt', { name: project.name })}
          className='shrink-0 rounded-lg'
        >
          <ProjectCover src={coverUrl} name={project.name} className='w-28 rounded-lg' />
        </ImagePreview>}
        <div className='min-w-0'>
          <h1 className='truncate text-3xl font-bold'>{project.name}</h1>
          {project.description && <p className='mt-1 max-w-2xl text-sm whitespace-pre-line opacity-70'>{
            project.description
          }</p>}
        </div>
      </div>
      <div className='shrink-0'>
        <ProjectActions
          project={project}
          sessionCount={sessions.length}
          onEdit={() => {
            setFormSession((session) => session + 1)
            editState.open()
          }}
        />
      </div>
    </div>

    <section>
      <h2 className='compact text-xl font-semibold'>{t('page.sessionsHeading')}</h2>
      {sessionsStatus === 'loading'
        ? <div className='grid place-items-center py-10'>
          <Spinner />
        </div>
        : <ProjectSessionsTable sessions={sessions} />}
    </section>

    <EditProjectModal
      key={formSession}
      state={editState}
      project={project}
    />
  </div>
}
