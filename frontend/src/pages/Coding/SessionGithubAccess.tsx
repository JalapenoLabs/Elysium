// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'
import type { GithubCredential, RepositoryAccess } from '../../api/routes/githubRoutes'

// Core
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// User interface
import { Spinner } from '@heroui/react'

// Utility
import { HTTPError } from 'ky'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { checkRepositoryAccess } from '../../api/routes/githubRoutes'
import { parseGithubRepository } from '../Settings/Github/githubPresentation'

type Props = {
  // The token the session would start with.
  credential: GithubCredential
  repositoryUrl: string
}

// What the chosen token can do with a repository added by URL, checked with GitHub. Only
// repositories on github.com are checked. A warning never blocks the session: GitHub may
// know better by the time the agent runs.
export function SessionGithubAccess(props: Props) {
  const { t } = useTranslation('coding')
  const repository = parseGithubRepository(props.repositoryUrl)

  const { data, error, isLoading } = useSWR(
    repository
      ? [ 'repository-access', props.credential.id, props.repositoryUrl ]
      : null,
    ([ , id, url ]) => checkRepositoryAccess(id, url),
    { revalidateOnFocus: false, shouldRetryOnError: false },
  )

  if (!repository) {
    return null
  }

  if (isLoading) {
    return <p className='flex items-center gap-2 text-sm opacity-70'>
      <Spinner size='sm' />
      <span>{t('create.github.access.checking', { repository })}</span>
    </p>
  }
  if (error) {
    // A refused token answers 400 with what to check; anything else is a failed check.
    const refusal = error instanceof HTTPError && error.response.status === 400
      ? getApiErrorMessage(error)
      : null
    if (!refusal) {
      console.debug('SessionGithubAccess could not check repository access', { error, repository })
    }
    return <p className='text-sm text-danger'>{
      refusal ?? t('create.github.access.failed', { repository })
    }</p>
  }
  if (data) {
    return accessMessage(data.result, t)
  }
  return null
}

function accessMessage(access: RepositoryAccess, t: TFunction<'coding'>) {
  const values = { repository: access.repository }
  if (!access.canRead) {
    return <p className='text-sm text-danger'>{t('create.github.access.none', values)}</p>
  }
  if (access.canPush === null) {
    return <p className='text-sm opacity-70'>{t('create.github.access.pushUnknown', values)}</p>
  }
  if (!access.canPush) {
    return <p className='text-sm text-warning'>{t('create.github.access.readOnly', values)}</p>
  }
  return <p className='text-sm text-success'>{t('create.github.access.push', values)}</p>
}
