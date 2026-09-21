// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'

// Core
import { useTranslation } from 'react-i18next'
import { useParams, useSearchParams } from 'react-router'
import useSWR from 'swr'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectJiraCredentialById } from '../../../store/jiraCredentialsSlice'

// User interface
import { ComboBox, Description, EmptyState, Input, Label, Link, ListBox, Spinner } from '@heroui/react'
import { JiraCredentialEditorLayout } from './JiraCredentialEditorLayout'
import { JiraDoneTransitionPanel } from './JiraDoneTransitionPanel'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { listJiraCredentialProjects } from '../../../api/routes/jiraRoutes'
import { useJiraCredentialsLoader } from '../../../hooks/useServerData'
import { DONE_TRANSITION_PROJECT_PARAM, UrlTree } from '../../../urls'

// `/settings/jira/:credentialId/done-transitions`: which done status resolving an item moves
// a project's issues into. A project with one done status needs nothing here; one with
// several waits for a choice, and its closes wait with it. `?project=` opens one project,
// such as from a close waiting on this choice.
export function JiraDoneTransitionsPage() {
  const { t } = useTranslation([ 'jira', 'common' ])
  const { credentialId = '' } = useParams()
  const [ searchParams, setSearchParams ] = useSearchParams()
  const projectKey = searchParams.get(DONE_TRANSITION_PROJECT_PARAM)
  const status = useJiraCredentialsLoader()
  const credential = useAppSelector((state) => selectJiraCredentialById(state, credentialId))

  const listing = useSWR(
    credential
      ? [ 'jira-credential-projects', credential.id ]
      : null,
    ([ , id ]) => listJiraCredentialProjects(id),
    { revalidateOnFocus: false, shouldRetryOnError: false },
  )
  const projects = (listing.data?.projects ?? []).filter((project) => project.selected)

  if (!credential) {
    if (status === 'loading') {
      return <div className='grid place-items-center py-16'>
        <Spinner />
      </div>
    }
    return <div className='container py-10 text-center text-sm'>
      <p className='compact opacity-70'>{t('page.notFound')}</p>
      <Link href={UrlTree.settingsJira} className='text-link'>{t('page.backToList')}</Link>
    </div>
  }

  function selectProject(key: Key | null) {
    if (key === null) {
      console.debug('JiraDoneTransitionsPage ignored an empty project selection')
      return
    }
    setSearchParams({ [DONE_TRANSITION_PROJECT_PARAM]: String(key) }, { replace: true })
  }

  return <JiraCredentialEditorLayout title={t('doneTransitions.title', { name: credential.name })}>
    <p className='relaxed max-w-2xl text-sm opacity-70'>{t('doneTransitions.hint')}</p>

    <ComboBox
      className='relaxed w-full max-w-md'
      isDisabled={!listing.data}
      selectedKey={projectKey}
      onSelectionChange={selectProject}
    >
      <Label>{t('doneTransitions.project')}</Label>
      <ComboBox.InputGroup>
        <Input placeholder={t('doneTransitions.projectPlaceholder')} />
        <ComboBox.Trigger />
      </ComboBox.InputGroup>
      {listing.error && <Description className='text-danger'>{
        getApiErrorMessage(listing.error) ?? t('scope.loadError')
      }</Description>}
      {listing.data?.truncated && <Description>{t('scope.truncated')}</Description>}
      <ComboBox.Popover>
        <ListBox renderEmptyState={() => <EmptyState>{t('scope.noMatches')}</EmptyState>}>{
          projects.map((project) => <ListBox.Item
            key={project.key}
            id={project.key}
            textValue={`${project.key} ${project.name}`}
          >
            {project.key}: {project.name}
            <ListBox.ItemIndicator />
          </ListBox.Item>)
        }</ListBox>
      </ComboBox.Popover>
    </ComboBox>

    {projectKey && <JiraDoneTransitionPanel
      // A new project starts from its own answer.
      key={projectKey}
      credentialId={credential.id}
      projectKey={projectKey}
    />}
  </JiraCredentialEditorLayout>
}
