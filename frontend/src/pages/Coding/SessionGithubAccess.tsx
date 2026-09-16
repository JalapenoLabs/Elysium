// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'
import type { GithubCredential, RepositoryAccess } from '../../api/routes/githubRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// User interface
import { Spinner } from '@heroui/react'

// Utility
import { HTTPError } from 'ky'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { checkRepositoryAccess } from '../../api/routes/githubRoutes'
import { REPOSITORY_ACCESS_CHECK_DELAY_MS } from '../../constants'
import { useDebouncedValue } from '../../hooks/useDebouncedValue'
import { parseGithubRepository } from '../Settings/Github/githubPresentation'

// A token expiring within this many days is worth a warning before a session starts on it.
const EXPIRY_WARNING_DAYS = 7
const MILLISECONDS_PER_DAY = 86_400_000

type Props = {
  // The token the session would start with, or null for none.
  credential: GithubCredential | null
  repositoryUrl: string
}

// What the chosen token can do with the session's repository, checked with GitHub as the
// repository field settles. Only repositories on github.com are checked. A warning never
// blocks the session: GitHub may know better by the time the agent runs.
export function SessionGithubAccess(props: Props) {
  const { t, i18n } = useTranslation('coding')
  const [ now ] = useState(() => Date.now())
  const repositoryUrl = useDebouncedValue(props.repositoryUrl.trim(), REPOSITORY_ACCESS_CHECK_DELAY_MS)
  const repository = parseGithubRepository(repositoryUrl)
  const credentialId = props.credential?.id

  const { data, error, isLoading } = useSWR(
    credentialId && repository
      ? [ 'repository-access', credentialId, repositoryUrl ]
      : null,
    ([ , id, url ]) => checkRepositoryAccess(id, url),
    { revalidateOnFocus: false, shouldRetryOnError: false },
  )

  if (!props.credential) {
    return null
  }

  const expiresAt = props.credential.tokenExpiresAt
    ? Date.parse(props.credential.tokenExpiresAt)
    : null
  if (expiresAt !== null && expiresAt <= now) {
    return <p className='text-sm text-danger'>{t('create.github.access.expired')}</p>
  }
  const expiryNote = expiresAt !== null && expiresAt - now <= EXPIRY_WARNING_DAYS * MILLISECONDS_PER_DAY
    ? <p className='text-sm text-warning'>{
      t('create.github.access.expiresSoon', {
        date: new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium' }).format(expiresAt),
      })
    }</p>
    : null

  if (!repository) {
    return expiryNote
  }

  let message = null
  if (isLoading) {
    message = <p className='flex items-center gap-2 text-sm opacity-70'>
      <Spinner size='sm' />
      <span>{t('create.github.access.checking', { repository })}</span>
    </p>
  }
  else if (error) {
    // A refused token answers 400 with what to check; anything else is a failed check.
    const refusal = error instanceof HTTPError && error.response.status === 400
      ? getApiErrorMessage(error)
      : null
    if (!refusal) {
      console.debug('SessionGithubAccess could not check repository access', { error, repository })
    }
    message = <p className='text-sm text-danger'>{
      refusal ?? t('create.github.access.failed', { repository })
    }</p>
  }
  else if (data) {
    message = accessMessage(data.result, t)
  }

  return <div className='flex flex-col gap-1'>
    {message}
    {expiryNote}
  </div>
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
