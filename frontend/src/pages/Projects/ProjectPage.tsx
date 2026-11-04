// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectProjectById } from '../../store/projectsSlice'

// User interface
import { Link, Spinner } from '@heroui/react'
import { ProjectDetails } from './ProjectDetails'

// Misc
import { useProjectsLoader } from '../../hooks/useServerData'
import { UrlTree } from '../../urls'

// `/projects/:projectId`: loads the project, then hands it to `ProjectDetails`, which can
// rely on it being there.
export function ProjectPage() {
  const { t } = useTranslation('projects')
  const { projectId = '' } = useParams()
  const projectsStatus = useProjectsLoader()
  const project = useAppSelector((state) => selectProjectById(state, projectId))

  if (project) {
    return <ProjectDetails project={project} />
  }

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
