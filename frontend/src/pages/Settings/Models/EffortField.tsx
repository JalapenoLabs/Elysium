// Copyright © 2026 Jalapeno Labs

import type { Effort } from '../../../api/routes/modelSettingRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { OptionSelect } from '../../../components/OptionSelect'

// Misc
import { effortLabelKeys } from './modelSettingsPresentation'

type Props = {
  // The trigger's id, stable so the field can be found and labelled.
  id: string
  // Read by screen readers; the column header says it to everyone else.
  label: string
  // The levels the API offers, lowest first.
  efforts: Effort[]
  value: Effort
  onChange: (effort: Effort) => void
}

// The effort a model runs at, in a table cell, from the levels the CLI's `--effort` takes.
export function EffortField(props: Props) {
  const { t } = useTranslation('models')
  const options = props.efforts.map((effort) => ({ id: effort, label: t(effortLabelKeys[effort]) }))

  return <OptionSelect
    isLabelHidden
    id={props.id}
    label={props.label}
    options={options}
    value={props.value}
    onChange={(value) => {
      const effort = props.efforts.find((option) => option === value)
      if (!effort) {
        console.debug('EffortField ignored an unknown effort', { value })
        return
      }
      props.onChange(effort)
    }}
  />
}
