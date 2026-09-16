// Copyright © 2026 Jalapeno Labs

import type { Project, ProjectGithub } from '../../api/routes/projectRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { selectAllGithubCredentials, selectDefaultGithubCredential } from '../../store/githubCredentialsSlice'
import { useAppDispatch, useAppSelector } from '../../store/hooks'
import { projectUpserted } from '../../store/projectsSlice'

// User interface
import { Alert, toast } from '@heroui/react'
import { GithubTokenSelect, INHERIT_GITHUB_TOKEN, NO_GITHUB_TOKEN } from '../../components/GithubTokenSelect'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { updateProject } from '../../api/routes/projectRoutes'
import { useGithubCredentialsLoader } from '../../hooks/useServerData'

type Props = {
  project: Project
}

// What the picker's two fixed choices save as. Every other key is a token id.
const githubByKey: Record<string, ProjectGithub> = {
  [INHERIT_GITHUB_TOKEN]: { access: 'default', credentialId: null },
  [NO_GITHUB_TOKEN]: { access: 'none', credentialId: null },
}

// The picker's key for a project's choice. A project whose token was deleted shows as
// following the default, which is what its sessions do.
function selectedKey(github: ProjectGithub) {
  if (github.access === 'none') {
    return NO_GITHUB_TOKEN
  }
  if (github.access === 'specific' && github.credentialId) {
    return github.credentialId
  }
  return INHERIT_GITHUB_TOKEN
}

// Which GitHub token this project's sessions start with, saved as soon as it changes. A
// session can still choose another when it is created.
export function ProjectGithubField(props: Props) {
  const { t } = useTranslation([ 'projects', 'common' ])
  const dispatch = useAppDispatch()
  useGithubCredentialsLoader()
  const credentials = useAppSelector(selectAllGithubCredentials)
  const workspaceDefault = useAppSelector(selectDefaultGithubCredential)
  const github = props.project.github

  // A deleted token leaves `specific` with no id: the project follows the default then.
  const isOrphaned = github.access === 'specific' && !github.credentialId

  async function save(key: string) {
    const next = githubByKey[key] ?? { access: 'specific', credentialId: key }

    try {
      const response = await updateProject(props.project.id, { github: next })
      dispatch(projectUpserted(response.project))
      toast.success(t('github.saved'))
    }
    catch (error) {
      console.debug('ProjectGithubField failed to save the GitHub token', { error, projectId: props.project.id })
      toast.danger(t('common:errors.unexpected'), { description: getApiErrorMessage(error) ?? undefined })
    }
  }

  return <section className='relaxed max-w-xl'>
    <h2 className='compact text-xl font-semibold'>{t('github.heading')}</h2>
    {isOrphaned && <Alert status='warning' className='compact'>
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Description>{t('github.orphaned')}</Alert.Description>
      </Alert.Content>
    </Alert>}
    <GithubTokenSelect
      label={t('github.label')}
      description={t('github.hint')}
      inheritLabel={workspaceDefault
        ? t('github.inherit', { name: workspaceDefault.name })
        : t('github.inheritNone')}
      value={selectedKey(github)}
      credentials={credentials}
      onChange={(key) => void save(key)}
    />
  </section>
}
