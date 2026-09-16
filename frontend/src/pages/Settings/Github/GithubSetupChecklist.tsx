// Copyright © 2026 Jalapeno Labs

import type { GithubTokenKind } from '../../../api/routes/githubRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { SetupChecklist } from '../../../components/SetupChecklist'

// Misc
import { githubSetupStepsByKind } from './githubPresentation'

type Props = {
  kind: GithubTokenKind
}

// Steps for creating a token of this kind, in GitHub's own words.
export function GithubSetupChecklist(props: Props) {
  const { t } = useTranslation('github')

  return <SetupChecklist
    title={t('setup.title')}
    description={t('setup.description')}
    steps={githubSetupStepsByKind[props.kind].map((step) => ({
      ...step,
      id: step.textKey,
      text: t(step.textKey),
    }))}
  />
}
