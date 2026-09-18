// Copyright © 2026 Jalapeno Labs

// Core
import { useNavigate, useParams } from 'react-router'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectJiraCredentialById } from '../../../store/jiraCredentialsSlice'

// User interface
import { Link, Spinner } from '@heroui/react'
import { JiraCredentialEditorLayout } from './JiraCredentialEditorLayout'
import { JiraCredentialForm } from './JiraCredentialForm'

// Misc
import { useJiraCredentialsLoader } from '../../../hooks/useServerData'
import { UrlTree } from '../../../urls'

// `/settings/jira/:credentialId/edit`: editing one Jira site.
export function EditJiraCredentialPage() {
  const { t } = useTranslation('jira')
  const navigate = useNavigate()
  const { credentialId = '' } = useParams()
  const status = useJiraCredentialsLoader()
  const credential = useAppSelector((state) => selectJiraCredentialById(state, credentialId))

  if (!credential) {
    if (status === 'loading') {
      return <div className='grid place-items-center py-16'>
        <Spinner />
      </div>
    }
    return <div className='container py-10 text-center text-sm'>
      <p className='compact opacity-70'>{t('page.notFound')}</p>
      <Link href={UrlTree.settingsJira} className='text-link'>{t('page.backToList')}</Link>
    </div>
  }

  return <JiraCredentialEditorLayout
    title={t('form.editTitle', { name: credential.name })}
  >
    <JiraCredentialForm
      // Remount when navigating between sites so the form takes the new values.
      key={credential.id}
      credential={credential}
      onSaved={() => navigate(UrlTree.settingsJira)}
      onCancel={() => navigate(UrlTree.settingsJira)}
    />
  </JiraCredentialEditorLayout>
}
