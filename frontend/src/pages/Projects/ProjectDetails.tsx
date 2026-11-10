// Copyright © 2026 Jalapeno Labs

import type { Project } from '../../api/routes/projectRoutes'

// Core
import { useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// Redux
import { selectIsCompanyMember } from '../../store/authSlice'
import { selectCodingSessionsByProjectId } from '../../store/codingSessionsSlice'
import { useAppSelector } from '../../store/hooks'

// User interface
import { InlineEditableText } from '../../components/InlineEditableText'
import { Page } from '../../layout/page/Page'
import { ProjectAutonomyField } from './ProjectAutonomyField'
import { ProjectBanner } from './ProjectBanner'
import { ProjectGithubField } from './ProjectGithubField'
import { ProjectLists } from './ProjectLists'

// Misc
import { useUserNames } from '../../hooks/useUserNames'
import { formatDate } from '../../format/dates'
import { UrlTree } from '../../urls'
import { DESCRIPTION_MAX_CHARACTERS, NAME_MAX_CHARACTERS } from './projectFormSchema'
import { useProjectActions } from './useProjectActions'
import { useProjectEditor } from './useProjectEditor'

type Props = {
  project: Project
}

// One project, edited in place and kept by the page's Save. The name is the page's title and
// is renamed where it is shown; the cover runs across the page below it, then what the
// project is about, its GitHub token, and its level of autonomy, and the Actions menu holds what cannot be done in
// place: removing the cover and deleting the project. Its lists follow, one per tab: action
// items, initiatives, the timeline, and coding sessions.
export function ProjectDetails(props: Props) {
  const { t } = useTranslation([ 'projects', 'common' ])
  const project = props.project
  const editor = useProjectEditor(project)
  const form = editor.form
  const [ name, description, coverFit, githubKey, autonomy ] = useWatch({
    control: form.control,
    name: [ 'name', 'description', 'coverFit', 'githubKey', 'autonomy' ],
  })
  // Deleting explains while sessions remain; the Sessions tab loads them.
  const sessions = useAppSelector((state) => selectCodingSessionsByProjectId(state, project.id))
  const userNames = useUserNames()
  const isMember = useAppSelector(selectIsCompanyMember)
  const actions = useProjectActions({ project, sessionCount: sessions.length })

  return <Page
    title={name}
    rename={{
      label: t('page.renameLabel'),
      maxLength: NAME_MAX_CHARACTERS,
      onChange: (value) => form.setValue('name', value, { shouldDirty: true }),
    }}
    save={{ save: editor.save, isDirty: editor.isDirty }}
    parents={[{ label: t('title'), href: UrlTree.projects }]}
    actions={actions}
  >
    <ProjectBanner
      project={project}
      fit={coverFit}
      onFitChange={(fit) => form.setValue('coverFit', fit, { shouldDirty: true })}
      coverFile={editor.coverFile}
      onCoverFileChange={editor.setCoverFile}
    />

    <section className='relaxed'>
      <h2 className='compact text-xl font-semibold'>{t('page.aboutHeading')}</h2>
      <div className='compact max-w-3xl'>
        <InlineEditableText
          id='project-description'
          value={description}
          label={t('page.describeLabel')}
          placeholder={t('page.addDescription')}
          isMultiline
          maxLength={DESCRIPTION_MAX_CHARACTERS}
          className='text-sm whitespace-pre-line opacity-80'
          onChange={(value) => form.setValue('description', value, { shouldDirty: true })}
        />
      </div>
      <p className='text-xs opacity-60'>{
        t('page.createdBy', {
          name: userNames[project.createdBy] ?? t('common:table.someone'),
          date: formatDate(project.createdAt),
        })
      }</p>
    </section>

    <ProjectGithubField
      project={project}
      value={githubKey}
      onChange={(key) => form.setValue('githubKey', key, { shouldDirty: true })}
    />

    <ProjectAutonomyField
      projectId={project.id}
      value={autonomy}
      isEditable={isMember}
      onChange={(level) => form.setValue('autonomy', level, { shouldDirty: true })}
    />

    <ProjectLists projectId={project.id} />
  </Page>
}
