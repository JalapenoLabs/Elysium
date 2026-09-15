// Copyright © 2026 Jalapeno Labs

import type { StorageProviderKind } from '../../../api/routes/storageRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { SetupChecklist } from '../../../components/SetupChecklist'

// Misc
import { storageSetupStepsByKind } from './storagePresentation'

type Props = {
  kind: StorageProviderKind
}

// Steps for finding a provider's settings and password, in the provider's own words.
export function StorageSetupChecklist(props: Props) {
  const { t } = useTranslation('storage')

  return <SetupChecklist
    title={t('setup.title')}
    description={t('setup.description')}
    steps={storageSetupStepsByKind[props.kind].map((step) => ({
      ...step,
      id: step.textKey,
      text: t(step.textKey),
    }))}
  />
}
