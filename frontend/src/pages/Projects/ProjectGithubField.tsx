// Copyright © 2026 Jalapeno Labs

import type { Project } from '../../api/routes/projectRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { selectAllGithubCredentials, selectDefaultGithubCredential } from '../../store/githubCredentialsSlice'
import { useAppSelector } from '../../store/hooks'

// User interface
import { Alert } from '@heroui/react'
import { GithubTokenSelect } from '../../components/GithubTokenSelect'

// Misc
import { useGithubCredentialsLoader } from '../../hooks/useServerData'

type Props = {
  project: Project
  // The picker's key (`toGithubKey`), as the page's form holds it.
  value: string
  onChange: (key: string) => void
}

// Which GitHub token this project's sessions start with, a field of the project page kept by
// its Save. A session can still choose another when it is created.
export function ProjectGithubField(props: Props) {
  const { t } = useTranslation([ 'projects', 'common' ])
  useGithubCredentialsLoader()
  const credentials = useAppSelector(selectAllGithubCredentials)
  const workspaceDefault = useAppSelector(selectDefaultGithubCredential)
  const github = props.project.github

  // A deleted token leaves `specific` with no id: the project follows the default then.
  const isOrphaned = github.access === 'specific' && !github.credentialId

  return <section className='relaxed max-w-xl'>
    <h2 className='compact text-xl font-semibold'>{t('github.heading')}</h2>
    {isOrphaned && <Alert status='warning' className='compact'>
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Description>{t('github.orphaned')}</Alert.Description>
      </Alert.Content>
    </Alert>}
    <GithubTokenSelect
      id='project-github-token'
      label={t('github.label')}
      description={t('github.hint')}
      inheritLabel={workspaceDefault
        ? t('github.inherit', { name: workspaceDefault.name })
        : t('github.inheritNone')}
      value={props.value}
      credentials={credentials}
      onChange={props.onChange}
    />
  </section>
}
