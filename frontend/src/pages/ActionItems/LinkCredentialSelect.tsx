// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectAllGithubCredentials } from '../../store/githubCredentialsSlice'
import { selectAllJiraCredentials } from '../../store/jiraCredentialsSlice'

// User interface
import { Link, Spinner } from '@heroui/react'
import { OptionSelect } from '../../components/OptionSelect'

// Misc
import { useGithubCredentialsLoader, useJiraCredentialsLoader } from '../../hooks/useServerData'
import { UrlTree } from '../../urls'
import { credentialOptions } from './linkPresentation'

type Props = {
  // A key from `toCredentialKey`, or empty until one is chosen.
  value: string
  onChange: (credentialKey: string) => void
}

// Every Jira site and GitHub token in one select, since a link or a container is reached
// through exactly one of them.
export function LinkCredentialSelect(props: Props) {
  const { t } = useTranslation('actionItems')
  const jiraStatus = useJiraCredentialsLoader()
  const githubStatus = useGithubCredentialsLoader()
  const jiraCredentials = useAppSelector(selectAllJiraCredentials)
  const githubCredentials = useAppSelector(selectAllGithubCredentials)

  if (jiraStatus === 'loading' || githubStatus === 'loading') {
    return <div className='grid place-items-center py-4'>
      <Spinner size='sm' />
    </div>
  }

  const options = credentialOptions(jiraCredentials, githubCredentials, {
    jira: t('links.providers.jira'),
    github: t('links.providers.github'),
  })

  if (!options.length) {
    return <p className='text-sm opacity-70'>
      {t('links.picker.noCredentials')}{' '}
      <Link href={UrlTree.settings} className='text-link'>{t('links.picker.openSettings')}</Link>
    </p>
  }

  return <OptionSelect
    label={t('links.picker.credential')}
    options={options}
    value={props.value}
    onChange={props.onChange}
  />
}
