// Copyright © 2026 Jalapeno Labs

// Core
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// UI
import { AuthNotice } from '../../components/auth/AuthNotice'

// Misc
import { resolveSession } from '../../auth/session'
import { UrlTree } from '../../urls'

// The identity service (API/Keycloak) is unreachable. A branded, reassuring
// degraded state; retry re-probes the session and, if it recovers, returns home.
export function IdentityUnavailable() {
  const { t } = useTranslation('auth')
  const navigate = useNavigate()

  return <AuthNotice
    state='error'
    title={t('fault.unavailable.title')}
    body={t('fault.unavailable.body')}
    actionLabel={t('fault.unavailable.action')}
    onAction={() => {
      void resolveSession()
      navigate(UrlTree.home)
    }}
  />
}
