// Copyright © 2026 Jalapeno Labs

// Core
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// UI
import { AuthNotice } from '../../components/auth/AuthNotice'

// Misc
import { UrlTree } from '../../urls'

// Authenticated, but the account isn't on a team with permission for this area.
export function AccessDenied() {
  const { t } = useTranslation('auth')
  const navigate = useNavigate()

  return <AuthNotice
    state='error'
    showPresence={false}
    title={t('fault.denied.title')}
    body={t('fault.denied.body')}
    actionLabel={t('fault.denied.action')}
    onAction={() => navigate(UrlTree.home)}
  />
}
