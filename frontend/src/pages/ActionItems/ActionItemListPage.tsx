// Copyright © 2026 Jalapeno Labs

import type { DataTableAction } from '@jalapenolabs/uikit'
import type { ActionItem } from '../../api/routes/actionItemRoutes'

// Core
import { useTranslation } from 'react-i18next'
import { useSearchParams } from 'react-router'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectDeletedActionItems, selectLiveActionItems } from '../../store/actionItemsSlice'

// User interface
import { Spinner } from '@heroui/react'
import { LuUndo2 } from 'react-icons/lu'
import { EmptyNotice } from '../../components/EmptyNotice'
import { ActionItemFiltersBar } from './ActionItemFiltersBar'
import { ActionItemTable } from './ActionItemTable'

// Misc
import { useActionItemsLoader, useDeletedActionItemsLoader } from '../../hooks/useServerData'
import { useNow } from '../../hooks/useNow'
import { filterActionItems, readActionItemFilters, writeActionItemFilters } from './actionItemFilters'
import { useActionItemActions } from './useActionItemActions'

// `/action-items/all`: every item in a table, filtered from the address so a filtered view
// survives opening an item and coming back. Deleted items show in place of the rest when
// asked for, each with Restore.
export function ActionItemListPage() {
  const { t } = useTranslation('actionItems')
  const actions = useActionItemActions()
  const [ searchParams, setSearchParams ] = useSearchParams()
  const filters = readActionItemFilters(searchParams)
  const now = useNow()
  const liveStatus = useActionItemsLoader()
  const deletedStatus = useDeletedActionItemsLoader(filters.deleted)
  const liveItems = useAppSelector(selectLiveActionItems)
  const deletedItems = useAppSelector(selectDeletedActionItems)

  const status = filters.deleted
    ? deletedStatus
    : liveStatus
  const items = filterActionItems(
    filters.deleted
      ? deletedItems
      : liveItems,
    filters,
    now,
  )

  // Only a deleted item has anything to do from its row. Restoring resolves once the answer is
  // in, so its button spins until then.
  const deletedRowActions: DataTableAction<ActionItem>[] = [{
    id: 'restore',
    label: t('list.restore'),
    icon: <LuUndo2 className='size-4' aria-hidden />,
    onPress: async (row) => {
      await actions.restore(row.original)
    },
  }]

  return <div>
    <ActionItemFiltersBar
      filters={filters}
      onChange={(next) => setSearchParams(writeActionItemFilters(next), { replace: true })}
    />

    {status === 'loading' && <div className='grid place-items-center py-16'>
      <Spinner />
    </div>}

    {status === 'failed' && <p className='py-10 text-center text-sm text-danger'>{t('list.loadError')}</p>}

    {status === 'loaded' && !items.length && <EmptyNotice>{
      filters.deleted
        ? t('list.emptyDeleted')
        : t('list.empty')
    }</EmptyNotice>}

    {status === 'loaded' && items.length > 0 && <ActionItemTable
      items={items}
      now={now}
      label={t('nav.all')}
      storageId={filters.deleted
        ? 'elysium.action-items.deleted.table.v1'
        : 'elysium.action-items.all.table.v1'}
      isSearchable
      rowActions={filters.deleted
        ? deletedRowActions
        : undefined}
    />}
  </div>
}
