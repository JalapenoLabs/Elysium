// Copyright © 2026 Jalapeno Labs

import type { GithubCredential } from '../../api/routes/githubRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// A token expiring within this many days is worth a warning before a session starts on it.
const EXPIRY_WARNING_DAYS = 7
const MILLISECONDS_PER_DAY = 86_400_000

type Props = {
  // The token the session would start with, or null for none.
  credential: GithubCredential | null
}

// Warns about a session's token that has expired or expires within a week. A warning never
// blocks the session.
export function SessionGithubTokenExpiry(props: Props) {
  const { t, i18n } = useTranslation('coding')
  const [ now ] = useState(() => Date.now())

  const expiresAt = props.credential?.tokenExpiresAt
    ? Date.parse(props.credential.tokenExpiresAt)
    : null
  if (expiresAt === null) {
    return null
  }

  if (expiresAt <= now) {
    return <p className='text-sm text-danger'>{t('create.github.access.expired')}</p>
  }
  if (expiresAt - now > EXPIRY_WARNING_DAYS * MILLISECONDS_PER_DAY) {
    return null
  }
  return <p className='text-sm text-warning'>{
    t('create.github.access.expiresSoon', {
      date: new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium' }).format(expiresAt),
    })
  }</p>
}
