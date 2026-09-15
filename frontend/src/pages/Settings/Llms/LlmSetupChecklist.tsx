// Copyright © 2026 Jalapeno Labs

import type { LlmType } from '../../../api/routes/llmRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { SetupChecklist } from '../../../components/SetupChecklist'

// Misc
import { llmSetupStepsByType } from './llmProviders'

type Props = {
  type: LlmType
}

// Steps for obtaining the secret token of one provider type.
export function LlmSetupChecklist(props: Props) {
  const { t } = useTranslation('llms')

  return <SetupChecklist
    title={t('setup.title')}
    description={t('setup.description')}
    steps={llmSetupStepsByType[props.type].map((step) => ({
      ...step,
      id: step.textKey,
      text: t(step.textKey),
    }))}
  />
}
