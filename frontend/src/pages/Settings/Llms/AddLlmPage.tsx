// Copyright © 2026 Jalapeno Labs

import type { LlmType } from '../../../api/routes/llmRoutes'

// Core
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// User interface
import { LlmEditorLayout } from './LlmEditorLayout'
import { LlmForm } from './LlmForm'

// Misc
import { UrlTree } from '../../../urls'
import { llmTypeLabelKeys } from './llmPresentation'

type Props = {
  type: LlmType
}

// `/settings/llms/add-<provider>`: one page per provider type, so its setup steps
// always match the token being pasted.
export function AddLlmPage(props: Props) {
  const { t } = useTranslation('llms')
  const navigate = useNavigate()

  return <LlmEditorLayout
    title={t('page.addTitle', { type: t(llmTypeLabelKeys[props.type]) })}
    type={props.type}
  >
    <LlmForm
      type={props.type}
      llm={null}
      onSaved={() => navigate(UrlTree.settingsLlms)}
      onCancel={() => navigate(UrlTree.settingsLlms)}
    />
  </LlmEditorLayout>
}
