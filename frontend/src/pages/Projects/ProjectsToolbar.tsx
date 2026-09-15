// Copyright © 2026 Jalapeno Labs

import type { Key, Selection } from '@heroui/react'
import type { ProjectSort, ProjectSortKey, ProjectView } from './projectListing'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Label, ListBox, SearchField, Select, ToggleButton, ToggleButtonGroup } from '@heroui/react'
import { LuArrowDownNarrowWide, LuArrowUpNarrowWide, LuLayoutGrid, LuTable2 } from 'react-icons/lu'

// Misc
import { PROJECT_SORT_KEYS } from './projectListing'

type Props = {
  search: string
  onSearchChange: (search: string) => void
  sort: ProjectSort
  onSortChange: (sort: ProjectSort) => void
  view: ProjectView
  onViewChange: (view: ProjectView) => void
  resultsCount: number
}

const sortLabelKeys = {
  name: 'sort.name',
  sessions: 'sort.sessions',
  updated: 'sort.updated',
} as const satisfies Record<ProjectSortKey, string>

// Search, sort, and the grid or tiles switch. It drives both views, so switching views
// keeps the same results in the same order.
export function ProjectsToolbar(props: Props) {
  const { t } = useTranslation('projects')

  const directionLabel = props.sort.descending
    ? t('toolbar.descending')
    : t('toolbar.ascending')

  function changeView(keys: Selection) {
    // The group disallows an empty selection, so one view is always selected.
    const [ view ] = keys === 'all'
      ? []
      : [ ...keys ]
    if (view === 'grid' || view === 'tiles') {
      props.onViewChange(view)
    }
  }

  return <div className='relaxed flex flex-wrap items-center gap-3'>
    <SearchField
      aria-label={t('toolbar.search')}
      value={props.search}
      onChange={props.onSearchChange}
    >
      <SearchField.Group>
        <SearchField.SearchIcon />
        <SearchField.Input className='w-64' placeholder={t('toolbar.search')} />
        <SearchField.ClearButton />
      </SearchField.Group>
    </SearchField>

    <span className='text-sm opacity-70'>{
      t('toolbar.results', { count: props.resultsCount })
    }</span>

    <div className='ml-auto flex flex-wrap items-center gap-3'>
      <Select
        className='w-48'
        aria-label={t('toolbar.sortBy')}
        value={props.sort.key}
        onChange={(key: Key | null) => props.onSortChange({ ...props.sort, key: key as ProjectSortKey })}
      >
        <Label className='sr-only'>{t('toolbar.sortBy')}</Label>
        <Select.Trigger>
          <Select.Value />
          <Select.Indicator />
        </Select.Trigger>
        <Select.Popover>
          <ListBox>{
            PROJECT_SORT_KEYS.map((sortKey) => <ListBox.Item
              key={sortKey}
              id={sortKey}
              textValue={t(sortLabelKeys[sortKey])}
            >
              {t(sortLabelKeys[sortKey])}
              <ListBox.ItemIndicator />
            </ListBox.Item>)
          }</ListBox>
        </Select.Popover>
      </Select>

      <Button
        isIconOnly
        variant='outline'
        aria-label={directionLabel}
        onPress={() => props.onSortChange({ ...props.sort, descending: !props.sort.descending })}
      >
        {props.sort.descending
          ? <LuArrowDownNarrowWide className='size-4' aria-hidden />
          : <LuArrowUpNarrowWide className='size-4' aria-hidden />}
      </Button>

      <ToggleButtonGroup
        aria-label={t('toolbar.view')}
        selectionMode='single'
        disallowEmptySelection
        selectedKeys={[ props.view ]}
        onSelectionChange={changeView}
      >
        <ToggleButton id='grid' aria-label={t('toolbar.grid')}>
          <LuTable2 className='size-4' aria-hidden />
          <span>{t('toolbar.grid')}</span>
        </ToggleButton>
        <ToggleButton id='tiles' aria-label={t('toolbar.tiles')}>
          <ToggleButtonGroup.Separator />
          <LuLayoutGrid className='size-4' aria-hidden />
          <span>{t('toolbar.tiles')}</span>
        </ToggleButton>
      </ToggleButtonGroup>
    </div>
  </div>
}
