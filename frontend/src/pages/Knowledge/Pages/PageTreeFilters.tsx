// Copyright © 2026 Jalapeno Labs

import type { PageTreeFilter } from './pageTree'

// Core
import { useId } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectLabelsInUse } from '../../../store/pagesSlice'

// User interface
import { LabelSelect } from './LabelSelect'

type Props = {
  filter: PageTreeFilter
  onChange: (filter: PageTreeFilter) => void
}

// Everything that narrows the tree, above it. With no label in use there is nothing to choose, so
// the panel stays as it was, unless a filter is on and needs its way off.
export function PageTreeFilters(props: Props) {
  const { t } = useTranslation('knowledge')
  const labels = useAppSelector(selectLabelsInUse)
  // The panel and the phone's drawer can both be mounted, each with its own field.
  const id = useId()

  if (!labels.length && !props.filter.label) {
    return null
  }

  return <div className='px-2.5'>
    <LabelSelect
      id={id}
      label={t('labels.filter')}
      labels={labels}
      value={props.filter.label}
      onChange={(label) => props.onChange({ ...props.filter, label })}
      isLabelHidden
    />
  </div>
}
