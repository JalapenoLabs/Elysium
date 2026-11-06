// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Autocomplete, EmptyState, Label, ListBox, SearchField, useFilter } from '@heroui/react'

// Misc
import { useInitialFocus } from '../hooks/useInitialFocus'

export type AddPickerOption = {
  id: string
  label: string
  // A second, smaller line, such as where the thing sits. What search matches includes it.
  detail?: string
}

type Props = {
  // The trigger's id; the search in its popover is `<id>-search`.
  id: string
  // What adding does, as the trigger reads while it holds nothing.
  label: string
  emptyLabel: string
  options: AddPickerOption[]
  onPick: (id: string) => void
  isDisabled?: boolean
  className?: string
}

// Adds one thing from a searchable list at once, such as a project to a page: it holds no value, so
// it reads as what it does again the moment one is picked, and closing it opens nothing back up. A
// ComboBox reopens when its text is cleared while it has focus.
export function AddPicker(props: Props) {
  const { t } = useTranslation('common')
  const { contains } = useFilter({ sensitivity: 'base' })
  // The popover mounts its search each time it opens, so typing narrows the list at once.
  const searchRef = useInitialFocus()

  return <Autocomplete
    id={props.id}
    className={props.className}
    selectionMode='single'
    isDisabled={props.isDisabled}
    placeholder={props.label}
    value={null}
    onChange={(key: Key | Key[] | null) => {
      if (key === null || Array.isArray(key)) {
        console.debug('AddPicker ignored an empty selection', { id: props.id, key })
        return
      }
      props.onPick(String(key))
    }}
  >
    <Label className='sr-only'>{props.label}</Label>
    <Autocomplete.Trigger>
      <Autocomplete.Value />
      <Autocomplete.Indicator />
    </Autocomplete.Trigger>
    <Autocomplete.Popover>
      <Autocomplete.Filter filter={contains}>
        <SearchField
          id={`${props.id}-search`}
          autoComplete='off'
          aria-label={t('picker.search')}
          variant='secondary'
        >
          <SearchField.Group>
            <SearchField.SearchIcon />
            <SearchField.Input ref={searchRef} placeholder={t('picker.search')} />
            <SearchField.ClearButton />
          </SearchField.Group>
        </SearchField>
        <ListBox renderEmptyState={() => <EmptyState>{props.emptyLabel}</EmptyState>}>{
          props.options.map((option) => <ListBox.Item
            key={option.id}
            id={option.id}
            textValue={[ option.detail, option.label ].filter(Boolean).join(' / ')}
          >
            <div className='min-w-0'>
              <p>{option.label}</p>
              {option.detail && <p className='text-xs opacity-70'>{option.detail}</p>}
            </div>
          </ListBox.Item>)
        }</ListBox>
      </Autocomplete.Filter>
    </Autocomplete.Popover>
  </Autocomplete>
}
