// Copyright © 2026 Jalapeno Labs

import type { Selection } from '@heroui/react'
import type { SessionsView } from './sessionsView'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { SearchField, ToggleButton, ToggleButtonGroup } from '@heroui/react'
import { LuLayoutGrid, LuTable2 } from 'react-icons/lu'

// Misc
import { SESSIONS_VIEWS } from './sessionsView'

type Props = {
  search: string
  onSearchChange: (search: string) => void
  resultsCount: number
  view: SessionsView
  onViewChange: (view: SessionsView) => void
}

// The Sessions panel's header: search, the result count, and the table or tiles switch.
// It drives both views, so switching keeps the same sessions listed.
export function SessionsToolbar(props: Props) {
  const { t } = useTranslation('coding')

  function changeView(keys: Selection) {
    // The group disallows an empty selection, so one view is always selected.
    const [ selected ] = keys === 'all'
      ? []
      : [ ...keys ]
    const view = SESSIONS_VIEWS.find((candidate) => candidate === selected)
    if (!view) {
      console.debug('The sessions view switch reported an unknown view', { selected })
      return
    }
    props.onViewChange(view)
  }

  return <div className='relaxed flex flex-wrap items-center gap-3'>
    <SearchField
      className='min-w-48 flex-1 @2xl:max-w-64'
      aria-label={t('sessions.toolbar.search')}
      value={props.search}
      onChange={props.onSearchChange}
    >
      <SearchField.Group>
        <SearchField.SearchIcon />
        <SearchField.Input className='w-full' placeholder={t('sessions.toolbar.search')} />
        <SearchField.ClearButton />
      </SearchField.Group>
    </SearchField>

    <span className='text-sm opacity-70'>{
      t('sessions.toolbar.results', { count: props.resultsCount })
    }</span>

    <ToggleButtonGroup
      className='ml-auto'
      aria-label={t('sessions.toolbar.view')}
      selectionMode='single'
      disallowEmptySelection
      selectedKeys={[ props.view ]}
      onSelectionChange={changeView}
    >
      <ToggleButton id='table' aria-label={t('sessions.toolbar.table')}>
        <LuTable2 className='size-4' aria-hidden />
        <span>{t('sessions.toolbar.table')}</span>
      </ToggleButton>
      <ToggleButton id='tiles' aria-label={t('sessions.toolbar.tiles')}>
        <ToggleButtonGroup.Separator />
        <LuLayoutGrid className='size-4' aria-hidden />
        <span>{t('sessions.toolbar.tiles')}</span>
      </ToggleButton>
    </ToggleButtonGroup>
  </div>
}
