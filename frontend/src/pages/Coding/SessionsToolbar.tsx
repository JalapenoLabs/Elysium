// Copyright © 2026 Jalapeno Labs

import type { Selection } from '@heroui/react'
import type { RefCallback } from 'react'
import type { SessionsView } from './sessionsView'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { ToggleButton, ToggleButtonGroup } from '@heroui/react'
import { LuLayoutGrid, LuTable2 } from 'react-icons/lu'
import { ListSearchField } from '../../components/ListSearchField'

// Misc
import { SESSIONS_VIEWS } from './sessionsView'

type Props = {
  search: string
  onSearchChange: (search: string) => void
  resultsCount: number
  view: SessionsView
  onViewChange: (view: SessionsView) => void
  // Receives the element the sessions table renders its filters, Columns, and export into.
  controlsRef: RefCallback<HTMLDivElement>
}

// The Sessions panel's titlebar: search, the result count, the table's filters, Columns, and
// export, and the table or tiles switch. Search and filters drive both views, so switching
// keeps the same sessions listed.
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
    <ListSearchField
      id='coding-sessions-search'
      label={t('sessions.toolbar.search')}
      value={props.search}
      onChange={props.onSearchChange}
    />

    <span className='text-sm opacity-70'>{
      t('sessions.toolbar.results', { count: props.resultsCount })
    }</span>

    {/* Holds no box of its own, so the table's controls line up with the rest. */}
    <div ref={props.controlsRef} className='contents' />

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
