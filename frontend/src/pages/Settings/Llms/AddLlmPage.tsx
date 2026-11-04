// Copyright © 2026 Jalapeno Labs

import type { LlmType } from '../../../api/routes/llmRoutes'

// Core
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// User interface
import { ClaudeSignInForm } from './ClaudeSignInForm'
import { LlmEditorLayout } from './LlmEditorLayout'
import { LlmForm } from './LlmForm'

// Misc
import { UrlTree } from '../../../urls'
import { isTokenTypedByType, llmTypeLabelKeys } from './llmPresentation'

type Props = {
  type: LlmType
}

// `/settings/llms/add-<provider>`: one page per provider type, so its setup steps
// always match the token being pasted. A Claude account is signed in instead, and has no
// token to paste.
export function AddLlmPage(props: Props) {
  const { t } = useTranslation('llms')
  const navigate = useNavigate()
  const backToList = () => navigate(UrlTree.settingsLlms)

  return <LlmEditorLayout
    title={t('page.addTitle', { type: t(llmTypeLabelKeys[props.type]) })}
    type={props.type}
  >
    {isTokenTypedByType[props.type]
      ? <LlmForm
        type={props.type}
        llm={null}
        onSaved={backToList}
      />
      : <ClaudeSignInForm
        onSaved={backToList}
      />}
  </LlmEditorLayout>
}
