// Copyright © 2026 Jalapeno Labs

import type { StorageOption } from './storagePresentation'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { SetupChecklist } from '../../../components/SetupChecklist'

// Misc
import { storageSetupStepsByOption } from './storagePresentation'

type Props = {
  option: StorageOption
}

// Steps for finding a provider's settings and secret, in the provider's own words.
export function StorageSetupChecklist(props: Props) {
  const { t } = useTranslation('storage')

  return <SetupChecklist
    id='storage-setup'
    title={t('setup.title')}
    steps={storageSetupStepsByOption[props.option].map((step) => ({
      ...step,
      id: step.textKey,
      text: t(step.textKey),
    }))}
  />
}
