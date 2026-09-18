// Copyright © 2026 Jalapeno Labs

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router'

// Redux
import { selectAllCodingSessions } from '../../store/codingSessionsSlice'
import { useAppDispatch, useAppSelector } from '../../store/hooks'
import { projectUpserted, selectProjectById } from '../../store/projectsSlice'

// User interface
import { Breadcrumbs, Link, Spinner, toast } from '@heroui/react'
import { InlineEditableText } from '../../components/InlineEditableText'
import { ProjectActions } from './ProjectActions'
import { ProjectBanner } from './ProjectBanner'
import { ProjectGithubField } from './ProjectGithubField'
import { ProjectSessionsTable } from './ProjectSessionsTable'
import { ProjectWork } from './ProjectWork'

// Utility
import { HTTPError } from 'ky'

// Misc
import { updateProject } from '../../api/routes/projectRoutes'
import { useCodingSessionsLoader, useProjectsLoader } from '../../hooks/useServerData'
import { UrlTree } from '../../urls'
import { DESCRIPTION_MAX_CHARACTERS, NAME_MAX_CHARACTERS } from './projectFormSchema'

// `/projects/:projectId`: one project, edited in place. The name and description come
// first and are edited where they are shown, the cover runs across the page below them, and the Actions menu holds
// what cannot be: removing the cover and deleting the project. Its action items and
// initiatives, then its coding sessions, follow.
export function ProjectPage() {
  const { t } = useTranslation([ 'projects', 'common' ])
  const dispatch = useAppDispatch()
  const { projectId = '' } = useParams()
  const projectsStatus = useProjectsLoader()
  const sessionsStatus = useCodingSessionsLoader()
  const project = useAppSelector((state) => selectProjectById(state, projectId))
  const allSessions = useAppSelector(selectAllCodingSessions)

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

  // Throws on failure, so the field stays open on what was typed.
  async function saveField(changes: { name: string } | { description: string }) {
    if (!project) {
      console.debug('ProjectPage saved a field with no project loaded')
      return
    }
    try {
      const response = await updateProject(project.id, changes)
      dispatch(projectUpserted(response.project))
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 409) {
        toast.danger(t('form.errors.nameTaken'))
      }
      else {
        console.debug('ProjectPage failed to save the project', { error, projectId: project.id })
        toast.danger(t('common:errors.unexpected'))
      }
      throw error
    }
  }

  return <div className='container'>
    <Breadcrumbs className='compact'>
      <Breadcrumbs.Item href={UrlTree.projects}>{t('title')}</Breadcrumbs.Item>
      <Breadcrumbs.Item>{project.name}</Breadcrumbs.Item>
    </Breadcrumbs>

    <div className='level relaxed items-start gap-4'>
      <div className='min-w-0 flex-1'>
        <InlineEditableText
          value={project.name}
          label={t('page.renameLabel')}
          isRequired
          maxLength={NAME_MAX_CHARACTERS}
          className='text-3xl font-bold'
          onSave={(name) => saveField({ name })}
        />
        <div className='mt-1 max-w-3xl'>
          <InlineEditableText
            value={project.description}
            label={t('page.describeLabel')}
            placeholder={t('page.addDescription')}
            isMultiline
            maxLength={DESCRIPTION_MAX_CHARACTERS}
            className='text-sm whitespace-pre-line opacity-80'
            onSave={(description) => saveField({ description })}
          />
        </div>
      </div>
      <div className='shrink-0'>
        <ProjectActions
          project={project}
          sessionCount={sessions.length}
        />
      </div>
    </div>

    <ProjectBanner project={project} />

    <ProjectGithubField project={project} />

    <ProjectWork projectId={project.id} />

    <section>
      <h2 className='compact text-xl font-semibold'>{t('page.sessionsHeading')}</h2>
      {sessionsStatus === 'loading'
        ? <div className='grid place-items-center py-10'>
          <Spinner />
        </div>
        : <ProjectSessionsTable sessions={sessions} />}
    </section>
  </div>
}
