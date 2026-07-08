// Copyright © 2026 Jalapeno Labs

// Core
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// UI
import { AuthNotice } from '../../components/auth/AuthNotice'

// Misc
import { UrlTree } from '../../urls'

// A signed invite that is expired, already used, or malformed.
export function InvalidInvite() {
  const { t } = useTranslation('auth')
  const navigate = useNavigate()

  return <AuthNotice
    state='error'
    title={t('invite.invalid.title')}
    body={t('invite.invalid.body')}
    actionLabel={t('invite.invalid.action')}
    onAction={() => navigate(UrlTree.signIn)}
  />
}
