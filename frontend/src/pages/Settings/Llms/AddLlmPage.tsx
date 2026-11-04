// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { Navigate, useNavigate, useParams } from 'react-router'

// User interface
import { ClaudeSignInForm } from './ClaudeSignInForm'
import { LlmEditorLayout } from './LlmEditorLayout'
import { LlmForm } from './LlmForm'

// Misc
import { UrlTree } from '../../../urls'
import { isTokenTypedByType, LLM_TYPES, llmTypeLabelKeys } from './llmPresentation'
import { llmSlugByType } from './llmProviders'

// `/settings/llms/new/:provider`: one page per provider type, so its setup steps always match
// the token being pasted. A Claude account is signed in instead, and has no token to paste. A
// provider this app does not know goes back to the list.
export function AddLlmPage() {
  const { t } = useTranslation('llms')
  const navigate = useNavigate()
  const { provider = '' } = useParams()
  const type = LLM_TYPES.find((candidate) => llmSlugByType[candidate] === provider)

  if (!type) {
    console.debug('AddLlmPage was opened for a provider it does not know', { provider })
    return <Navigate to={UrlTree.settingsLlms} replace />
  }
  const backToList = () => navigate(UrlTree.settingsLlms)

  return <LlmEditorLayout
    title={t('page.addTitle', { type: t(llmTypeLabelKeys[type]) })}
    type={type}
    help={isTokenTypedByType[type]
      ? undefined
      : t('signIn.help')}
  >
    {isTokenTypedByType[type]
      ? <LlmForm
        // A new form per provider, so nothing typed for one carries over to another.
        key={type}
        type={type}
        llm={null}
        onSaved={backToList}
      />
      : <ClaudeSignInForm
        onSaved={backToList}
      />}
  </LlmEditorLayout>
}
