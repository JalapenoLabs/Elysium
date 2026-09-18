// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'

// Core
import { useMemo } from 'react'
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

// One thing the token reaches, ready to show.
export type JiraScopeOption = {
  id: string
  // What the user reads: a project's or a board's name.
  label: string
  // A short prefix shown before the label, such as a project key; null when there is none.
  detail: string | null
}

type Props = {
  label: string
  description: string
  options: JiraScopeOption[]
  selectedIds: string[]
  onChange: (ids: string[]) => void
  isDisabled: boolean
}

// Picks from what Jira said the token reaches, as removable tags under a searchable
// dropdown. There is no freeform entry anywhere: every option came from Jira.
export function JiraScopePicker(props: Props) {
  const { t } = useTranslation('jira')
  const { contains } = useFilter({ sensitivity: 'base' })

  const optionsById = useMemo(
    () => new Map(props.options.map((option) => [ option.id, option ])),
    [ props.options ],
  )

  return <Autocomplete
    selectionMode='multiple'
    isDisabled={props.isDisabled}
    placeholder={t('scope.placeholder')}
    value={props.selectedIds}
    onChange={(keys: Key | Key[] | null) => props.onChange(Array.isArray(keys)
      ? keys.map(String)
      : [])}
  >
    <Label>{props.label}</Label>
    <Autocomplete.Trigger>
      <Autocomplete.Value>{
        ({ defaultChildren, isPlaceholder }) => {
          if (isPlaceholder || !props.selectedIds.length) {
            return defaultChildren
          }
          return <TagGroup
            size='sm'
            aria-label={props.label}
            onRemove={(removed) => props.onChange(
              props.selectedIds.filter((id) => !removed.has(id)),
            )}
          >
            <TagGroup.List>{
              props.selectedIds.map((id) => {
                const option = optionsById.get(id)
                if (!option) {
                  console.debug('JiraScopePicker has no option for a selected id', { id })
                  return null
                }
                return <Tag key={id} id={id}>{option.detail ?? option.label}</Tag>
              })
            }</TagGroup.List>
          </TagGroup>
        }
      }</Autocomplete.Value>
      <Autocomplete.Indicator />
    </Autocomplete.Trigger>
    <Description>{props.description}</Description>
    <Autocomplete.Popover>
      <Autocomplete.Filter filter={contains}>
        <SearchField autoFocus aria-label={t('scope.search')} variant='secondary'>
          <SearchField.Group>
            <SearchField.SearchIcon />
            <SearchField.Input placeholder={t('scope.search')} />
            <SearchField.ClearButton />
          </SearchField.Group>
        </SearchField>
        <ListBox renderEmptyState={() => <EmptyState>{t('scope.noMatches')}</EmptyState>}>{
          props.options.map((option) => <ListBox.Item
            key={option.id}
            id={option.id}
            // Searching finds a project by its key as readily as by its name.
            textValue={option.detail
              ? `${option.detail} ${option.label}`
              : option.label}
          >
            <span className='flex items-center gap-2'>
              {option.detail && <code className='text-xs opacity-70'>{option.detail}</code>}
              <span>{option.label}</span>
            </span>
            <ListBox.ItemIndicator />
          </ListBox.Item>)
        }</ListBox>
      </Autocomplete.Filter>
    </Autocomplete.Popover>
  </Autocomplete>
}
