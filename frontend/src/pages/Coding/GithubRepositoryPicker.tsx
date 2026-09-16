// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { ReactNode } from 'react'
import type { GithubRepository } from '../../api/routes/githubRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import {
  Autocomplete,
  Chip,
  Description,
  EmptyState,
  Label,
  ListBox,
  SearchField,
  useFilter,
} from '@heroui/react'

type Props = {
  // Most recently pushed first, as the API lists them.
  repositories: GithubRepository[]
  // The full names of the repositories already chosen.
  selectedFullNames: string[]
  isDisabled: boolean
  description: ReactNode
  onChange: (fullNames: string[]) => void
}

// A searchable list of the repositories a token can see, picked many at a time. The chosen
// repositories are shown as rows below the picker, so its own value only counts them.
export function GithubRepositoryPicker(props: Props) {
  const { t } = useTranslation('coding')
  const { contains } = useFilter({ sensitivity: 'base' })

  return <Autocomplete
    selectionMode='multiple'
    isDisabled={props.isDisabled}
    placeholder={t('create.repositories.picker.placeholder')}
    value={props.selectedFullNames}
    onChange={(keys: Key | Key[] | null) => props.onChange(Array.isArray(keys)
      ? keys.map(String)
      : [])}
  >
    <Label>{t('create.repositories.label')}</Label>
    <Autocomplete.Trigger>
      <Autocomplete.Value>{
        ({ defaultChildren, isPlaceholder }) => isPlaceholder || !props.selectedFullNames.length
          ? defaultChildren
          : t('create.repositories.picker.selected', { count: props.selectedFullNames.length })
      }</Autocomplete.Value>
      <Autocomplete.Indicator />
    </Autocomplete.Trigger>
    <Description>{props.description}</Description>
    <Autocomplete.Popover>
      <Autocomplete.Filter filter={contains}>
        <SearchField autoFocus aria-label={t('create.repositories.picker.search')} variant='secondary'>
          <SearchField.Group>
            <SearchField.SearchIcon />
            <SearchField.Input placeholder={t('create.repositories.picker.search')} />
            <SearchField.ClearButton />
          </SearchField.Group>
        </SearchField>
        <ListBox renderEmptyState={() => <EmptyState>{t('create.repositories.picker.noMatches')}</EmptyState>}>{
          props.repositories.map((repository) => <ListBox.Item
            key={repository.fullName}
            id={repository.fullName}
            textValue={repository.fullName}
          >
            <span className='flex min-w-0 items-center gap-2'>
              <span className='truncate'>{repository.fullName}</span>
              {repository.private && <Chip size='sm' variant='soft'>{
                t('create.repositories.private')
              }</Chip>}
              {repository.archived && <Chip size='sm' variant='soft' color='warning'>{
                t('create.repositories.archived')
              }</Chip>}
            </span>
            <ListBox.ItemIndicator />
          </ListBox.Item>)
        }</ListBox>
      </Autocomplete.Filter>
    </Autocomplete.Popover>
  </Autocomplete>
}
