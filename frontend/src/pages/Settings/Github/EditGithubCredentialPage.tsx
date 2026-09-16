// Copyright © 2026 Jalapeno Labs

// Core
import { useNavigate, useParams } from 'react-router'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectGithubCredentialById } from '../../../store/githubCredentialsSlice'

// User interface
import { Link, Spinner } from '@heroui/react'
import { GithubCredentialEditorLayout } from './GithubCredentialEditorLayout'
import { GithubCredentialForm } from './GithubCredentialForm'

// Misc
import { useGithubCredentialsLoader } from '../../../hooks/useServerData'
import { UrlTree } from '../../../urls'

// `/settings/github/:credentialId/edit`: editing one GitHub token.
export function EditGithubCredentialPage() {
  const { t } = useTranslation('github')
  const navigate = useNavigate()
  const { credentialId = '' } = useParams()
  const status = useGithubCredentialsLoader()
  const credential = useAppSelector((state) => selectGithubCredentialById(state, credentialId))

  if (!credential) {
    if (status === 'loading') {
      return <div className='grid place-items-center py-16'>
        <Spinner />
      </div>
    }
    return <div className='container py-10 text-center text-sm'>
      <p className='compact opacity-70'>{t('page.notFound')}</p>
      <Link href={UrlTree.settingsGithub} className='text-link'>{t('page.backToList')}</Link>
    </div>
  }

  return <GithubCredentialEditorLayout
    title={t('form.editTitle', { name: credential.name })}
  >
    <GithubCredentialForm
      // Remount when navigating between credentials so the form takes the new values.
      key={credential.id}
      credential={credential}
      onSaved={() => navigate(UrlTree.settingsGithub)}
      onCancel={() => navigate(UrlTree.settingsGithub)}
    />
  </GithubCredentialEditorLayout>
}
