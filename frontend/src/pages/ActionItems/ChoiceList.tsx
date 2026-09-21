// Copyright © 2026 Jalapeno Labs

import type { Selection } from '@heroui/react'
import type { LoadStatus } from '../../hooks/useServerData'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Description, EmptyState, Label, ListBox, Spinner } from '@heroui/react'

// Misc
import { getApiErrorMessage } from '../../api/errors'

export type Choice = {
  id: string
  label: string
  // A second, quieter line, such as an issue's status.
  detail?: string
}

type Props = {
  label: string
  choices: Choice[]
  status: LoadStatus
  error: unknown
  // The provider held more than it listed.
  truncated: boolean
  selectedId: string | null
  onSelect: (id: string | null) => void
}

// One pick from a list a provider answered: an issue, a pull request, a milestone. Loading,
// a refusal, and an empty answer each read as such, so a picker never looks stuck.
export function ChoiceList(props: Props) {
  const { t } = useTranslation([ 'actionItems', 'common' ])

  if (props.status === 'loading') {
    return <div className='grid place-items-center py-6'>
      <Spinner size='sm' />
    </div>
  }

  if (props.status === 'failed') {
    const message = getApiErrorMessage(props.error) ?? t('common:errors.unexpected')
    return <p className='text-sm text-danger'>{t('links.picker.loadError', { error: message })}</p>
  }

  const selectedKeys = props.selectedId
    ? [ props.selectedId ]
    : []

  function select(keys: Selection) {
    // Single selection answers a set of at most one key; "all" never happens here.
    if (keys === 'all') {
      console.debug('ChoiceList ignored a select-all', { label: props.label })
      return
    }
    const [ first ] = keys
    props.onSelect(first === undefined
      ? null
      : String(first))
  }

  return <div className='flex flex-col gap-1'>
    <Label>{props.label}</Label>
    <div className='max-h-72 overflow-y-auto rounded-xl bg-surface'>
      <ListBox
        aria-label={props.label}
        selectionMode='single'
        selectedKeys={selectedKeys}
        onSelectionChange={select}
        renderEmptyState={() => <EmptyState>{t('links.picker.noResults')}</EmptyState>}
      >{
          props.choices.map((choice) => <ListBox.Item key={choice.id} id={choice.id} textValue={choice.label}>
            <span className='flex min-w-0 flex-col'>
              <span className='truncate'>{choice.label}</span>
              {choice.detail && <span className='truncate text-xs opacity-60'>{choice.detail}</span>}
            </span>
            <ListBox.ItemIndicator />
          </ListBox.Item>)
        }</ListBox>
    </div>
    {props.truncated && <Description>{t('links.picker.truncated')}</Description>}
  </div>
}
