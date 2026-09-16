// Copyright © 2026 Jalapeno Labs

// Core
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// User interface
import { GithubCredentialEditorLayout } from './GithubCredentialEditorLayout'
import { GithubCredentialForm } from './GithubCredentialForm'

// Misc
import { UrlTree } from '../../../urls'

// `/settings/github/new`: adding a GitHub token.
export function AddGithubCredentialPage() {
  const { t } = useTranslation('github')
  const navigate = useNavigate()

  return <GithubCredentialEditorLayout
    title={t('form.createTitle')}
  >
    <GithubCredentialForm
      credential={null}
      onSaved={() => navigate(UrlTree.settingsGithub)}
      onCancel={() => navigate(UrlTree.settingsGithub)}
    />
  </GithubCredentialEditorLayout>
}
