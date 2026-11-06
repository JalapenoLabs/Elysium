// Copyright © 2026 Jalapeno Labs

import type { PageTreeFilter } from './pageTree'

// Core
import { useId } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectLabelsInUse } from '../../../store/pagesSlice'

// User interface
import { SearchField } from '@heroui/react'
import { LabelSelect } from './LabelSelect'

type Props = {
  filter: PageTreeFilter
  onChange: (filter: PageTreeFilter) => void
}

// Everything that narrows the tree, above it: its titles as they are typed, and a label. With no
// label in use there is nothing to choose, so that stays out of the way, unless a filter is on and
// needs its way off.
export function PageTreeFilters(props: Props) {
  const { t } = useTranslation('knowledge')
  const labels = useAppSelector(selectLabelsInUse)
  // The panel and the phone's drawer can both be mounted, each with its own fields.
  const id = useId()

  return <div className='flex flex-col gap-2 px-2.5'>
    <SearchField
      id={`${id}-title`}
      autoComplete='off'
      aria-label={t('pages.filterByTitle')}
      value={props.filter.title}
      onChange={(title) => props.onChange({ ...props.filter, title })}
    >
      <SearchField.Group>
        <SearchField.SearchIcon />
        <SearchField.Input placeholder={t('pages.filterByTitle')} />
        <SearchField.ClearButton />
      </SearchField.Group>
    </SearchField>
    {(labels.length > 0 || props.filter.label) && <LabelSelect
      id={id}
      label={t('labels.filter')}
      labels={labels}
      value={props.filter.label}
      onChange={(label) => props.onChange({ ...props.filter, label })}
      isLabelHidden
    />}
  </div>
}
