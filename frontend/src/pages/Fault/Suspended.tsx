// Copyright © 2026 Jalapeno Labs

// Core
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// UI
import { AuthNotice } from '../../components/auth/AuthNotice'

// Misc
import { UrlTree } from '../../urls'

// Brute-force lockout or an admin suspension.
export function Suspended() {
  const { t } = useTranslation('auth')
  const navigate = useNavigate()

  return <AuthNotice
    state='error'
    showPresence={false}
    title={t('fault.suspended.title')}
    body={t('fault.suspended.body')}
    actionLabel={t('fault.suspended.action')}
    onAction={() => navigate(UrlTree.signIn)}
  />
}
