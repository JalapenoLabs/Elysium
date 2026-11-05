// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { PageTree } from './pageTree'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Autocomplete, EmptyState, Label, ListBox, SearchField, useFilter } from '@heroui/react'

// Misc
import { useInitialFocus } from '../../../hooks/useInitialFocus'
import { listPageDestinations } from './pageTree'

// The top of the tree, which has no page to name.
const TOP_OF_TREE = 'top'

type Props = {
  id: string
  tree: PageTree
  // The chosen page's id, or '' for the top of the tree.
  value: string
  onChange: (parentId: string) => void
}

// Where a new page goes: any page, found by its path through the tree, or the top.
export function PageParentPicker(props: Props) {
  const { t } = useTranslation('knowledge')
  const { contains } = useFilter({ sensitivity: 'base' })
  // The popover mounts its search each time it opens, so typing narrows the list at once.
  const searchRef = useInitialFocus()
  const options = [
    { id: TOP_OF_TREE, label: t('pages.editor.topOfTree') },
    ...listPageDestinations(props.tree, null).map((destination) => ({
      id: destination.id,
      label: [ ...destination.ancestorTitles, destination.title ].join(' / '),
    })),
  ]

  return <Autocomplete
    id={props.id}
    selectionMode='single'
    value={props.value || TOP_OF_TREE}
    onChange={(key: Key | Key[] | null) => {
      if (key === null || Array.isArray(key)) {
        console.debug('PageParentPicker ignored an empty selection', { key })
        return
      }
      const chosen = String(key)
      props.onChange(chosen === TOP_OF_TREE
        ? ''
        : chosen)
    }}
  >
    <Label>{t('pages.editor.parent')}</Label>
    <Autocomplete.Trigger>
      <Autocomplete.Value />
      <Autocomplete.Indicator />
    </Autocomplete.Trigger>
    <Autocomplete.Popover>
      <Autocomplete.Filter filter={contains}>
        <SearchField
          id={`${props.id}-search`}
          autoComplete='off'
          aria-label={t('pages.editor.parentSearch')}
          variant='secondary'
        >
          <SearchField.Group>
            <SearchField.SearchIcon />
            <SearchField.Input ref={searchRef} placeholder={t('pages.editor.parentSearch')} />
            <SearchField.ClearButton />
          </SearchField.Group>
        </SearchField>
        <ListBox renderEmptyState={() => <EmptyState>{t('pages.editor.parentNone')}</EmptyState>}>{
          options.map((option) => <ListBox.Item
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
