// Copyright © 2026 Jalapeno Labs

// Core
import { useNavigate, useParams } from 'react-router'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectLlmById } from '../../../store/llmsSlice'

// User interface
import { Link, Spinner } from '@heroui/react'
import { LlmEditorLayout } from './LlmEditorLayout'
import { LlmForm } from './LlmForm'

// Misc
import { useLlmsLoader } from '../../../hooks/useServerData'
import { UrlTree } from '../../../urls'

// `/settings/llms/:llmId/edit`. The provider type is fixed here; a different provider
// is a new credential, added from its own page.
export function EditLlmPage() {
  const { t } = useTranslation('llms')
  const navigate = useNavigate()
  const { llmId = '' } = useParams()
  const status = useLlmsLoader()
  const llm = useAppSelector((state) => selectLlmById(state, llmId))

  if (!llm) {
    if (status === 'loading') {
      return <div className='grid place-items-center py-16'>
        <Spinner />
      </div>
    }
    return <div className='container py-10 text-center text-sm'>
      <p className='compact opacity-70'>{t('page.notFound')}</p>
      <Link href={UrlTree.settingsLlms} className='text-link'>{t('page.backToList')}</Link>
    </div>
  }

  return <LlmEditorLayout
    title={t('page.editTitle', { name: llm.name })}
    type={llm.type}
  >
    <LlmForm
      // Remount when navigating between credentials so the form takes the new values.
      key={llm.id}
      type={llm.type}
      llm={llm}
      onSaved={() => navigate(UrlTree.settingsLlms)}
      onCancel={() => navigate(UrlTree.settingsLlms)}
    />
  </LlmEditorLayout>
}
