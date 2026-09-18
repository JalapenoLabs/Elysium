// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import {
  Autocomplete,
  Description,
  EmptyState,
  Label,
  ListBox,
  SearchField,
  Tag,
  TagGroup,
  useFilter,
} from '@heroui/react'

export type PickerOption = {
  id: string
  label: string
}

type Props = {
  label: string
  // Shown in the search field and when nothing matches.
  searchLabel: string
  emptyLabel: string
  placeholder?: string
  description?: string
  options: PickerOption[]
  value: string[]
  onChange: (value: string[]) => void
  isDisabled?: boolean
}

// Picks any number of things, such as projects or initiatives, as removable tags from a
// searchable list.
export function MultiPicker(props: Props) {
  const { t } = useTranslation('common')
  const { contains } = useFilter({ sensitivity: 'base' })

  const labelsById = new Map(props.options.map((option) => [ option.id, option.label ]))

  return <Autocomplete
    selectionMode='multiple'
    isDisabled={props.isDisabled}
    placeholder={props.placeholder ?? t('picker.placeholder')}
    value={props.value}
    onChange={(keys: Key | Key[] | null) => props.onChange(Array.isArray(keys)
      ? keys.map(String)
      : [])}
  >
    <Label>{props.label}</Label>
    <Autocomplete.Trigger>
      <Autocomplete.Value>{
        ({ defaultChildren, isPlaceholder }) => {
          if (isPlaceholder || !props.value.length) {
            return defaultChildren
          }
          return <TagGroup
            size='sm'
            aria-label={props.label}
            onRemove={(removed) => props.onChange(props.value.filter((id) => !removed.has(id)))}
          >
            <TagGroup.List>{
              props.value.map((id) => {
                const label = labelsById.get(id)
                if (!label) {
                  console.debug('MultiPicker has no option for a selected id', { id })
                  return null
                }
                return <Tag key={id} id={id}>{label}</Tag>
              })
            }</TagGroup.List>
          </TagGroup>
        }
      }</Autocomplete.Value>
      <Autocomplete.Indicator />
    </Autocomplete.Trigger>
    {props.description && <Description>{props.description}</Description>}
    <Autocomplete.Popover>
      <Autocomplete.Filter filter={contains}>
        <SearchField autoFocus aria-label={props.searchLabel} variant='secondary'>
          <SearchField.Group>
            <SearchField.SearchIcon />
            <SearchField.Input placeholder={props.searchLabel} />
            <SearchField.ClearButton />
          </SearchField.Group>
        </SearchField>
        <ListBox renderEmptyState={() => <EmptyState>{props.emptyLabel}</EmptyState>}>{
          props.options.map((option) => <ListBox.Item
            key={option.id}
            id={option.id}
            textValue={option.label}
          >
            {option.label}
            <ListBox.ItemIndicator />
          </ListBox.Item>)
        }</ListBox>
      </Autocomplete.Filter>
    </Autocomplete.Popover>
  </Autocomplete>
}
