// Copyright © 2026 Jalapeno Labs

// Core
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// UI
import { AuthNotice } from '../../components/auth/AuthNotice'

// Misc
import { UrlTree } from '../../urls'

// Confirmed sign-out, with a clear way back in.
export function SignedOut() {
  const { t } = useTranslation('auth')
  const navigate = useNavigate()

  return <AuthNotice
    state='idle'
    title={t('signedOut.title')}
    body={t('signedOut.body')}
    actionLabel={t('signedOut.action')}
    onAction={() => navigate(UrlTree.signIn)}
  />
}
