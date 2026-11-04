// Copyright © 2026 Jalapeno Labs

import type { Project } from '../../api/routes/projectRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { selectCodingSessionsByProjectId } from '../../store/codingSessionsSlice'
import { useAppDispatch, useAppSelector } from '../../store/hooks'
import { projectUpserted } from '../../store/projectsSlice'

// User interface
import { Spinner } from '@heroui/react'
import { InlineEditableText } from '../../components/InlineEditableText'
import { Page } from '../../layout/page/Page'
import { CodingSessionsTable } from '../Coding/CodingSessionsTable'
import { ProjectBanner } from './ProjectBanner'
import { ProjectGithubField } from './ProjectGithubField'
import { ProjectTimeline } from './ProjectTimeline'
import { ProjectWork } from './ProjectWork'

// Utility
import { HTTPError } from 'ky'

// Misc
import { updateProject } from '../../api/routes/projectRoutes'
import { useCodingSessionsLoader } from '../../hooks/useServerData'
import { useUserNames } from '../../hooks/useUserNames'
import { formatDate } from '../../format/dates'
import { UrlTree } from '../../urls'
import { DESCRIPTION_MAX_CHARACTERS, NAME_MAX_CHARACTERS } from './projectFormSchema'
import { useProjectActions } from './useProjectActions'
import { toast } from '../../toast/toast'

type Props = {
  project: Project
}

// One project, edited in place. The name is the page's title and is renamed where it is
// shown; the cover runs across the page below it, then what the project is about, and the
// Actions menu holds what cannot be done in place: removing the cover and deleting the
// project. Its action items and initiatives, its timeline, then its coding sessions, follow.
export function ProjectDetails(props: Props) {
  const { t } = useTranslation([ 'projects', 'common' ])
  const dispatch = useAppDispatch()
  const project = props.project
  const sessionsStatus = useCodingSessionsLoader()
  const sessions = useAppSelector((state) => selectCodingSessionsByProjectId(state, project.id))
  const userNames = useUserNames()
  const actions = useProjectActions({ project, sessionCount: sessions.length })

  // Throws on failure, so the field stays open on what was typed.
  async function saveField(changes: { name: string } | { description: string }) {
    try {
      const response = await updateProject(project.id, changes)
      dispatch(projectUpserted(response.project))
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 409) {
        toast.danger(t('form.errors.nameTaken'))
      }
      else {
        console.debug('ProjectDetails failed to save the project', { error, projectId: project.id })
        toast.danger(t('common:errors.unexpected'))
      }
      throw error
    }
  }

  return <Page
    title={project.name}
    rename={{
      label: t('page.renameLabel'),
      maxLength: NAME_MAX_CHARACTERS,
      onSave: (name) => saveField({ name }),
    }}
    parents={[{ label: t('title'), href: UrlTree.projects }]}
    actions={actions}
  >
    <ProjectBanner project={project} />

    <section className='relaxed'>
      <h2 className='compact text-xl font-semibold'>{t('page.aboutHeading')}</h2>
      <div className='compact max-w-3xl'>
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
      <p className='text-xs opacity-60'>{
        t('page.createdBy', {
          name: userNames[project.createdBy] ?? t('common:table.someone'),
          date: formatDate(project.createdAt),
        })
      }</p>
    </section>

    <ProjectGithubField project={project} />

    <ProjectWork projectId={project.id} />

    <ProjectTimeline projectId={project.id} />

    <section>
      <h2 className='compact text-xl font-semibold'>{t('page.sessionsHeading')}</h2>
      {sessionsStatus === 'loading'
        ? <div className='grid place-items-center py-10'>
          <Spinner />
        </div>
        : <CodingSessionsTable
          sessions={sessions}
          ids={{
            tableElementId: 'project-sessions-table',
            tableLocalStorageId: 'elysium.projects.sessions.table.v2',
          }}
          ariaLabel={t('page.sessionsHeading')}
          emptyMessage={t('page.sessionsEmpty')}
        />}
    </section>
  </Page>
}
