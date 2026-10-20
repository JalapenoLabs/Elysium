// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { shallowEqual } from 'react-redux'
import { useSearchParams } from 'react-router'

// Redux
import { selectLatestSessionsByStudioItemId } from '../../store/codingSessionsSlice'
import { useAppSelector } from '../../store/hooks'
import { selectAllStorageLocations } from '../../store/storageLocationsSlice'
import { selectDeletedStudioItems, selectLiveStudioItems } from '../../store/studioItemsSlice'

// User interface
import { Button, Spinner, useOverlayState } from '@heroui/react'
import { LuPlus } from 'react-icons/lu'
import { EmptyNotice } from '../../components/EmptyNotice'
import { CreateStudioItemModal } from './CreateStudioItemModal'
import { StudioFiltersBar } from './StudioFiltersBar'
import { StudioItemTile } from './StudioItemTile'
import { StudioNoStorage } from './StudioNoStorage'

// Misc
import {
  useDeletedStudioItemsLoader,
  useSatellitesLoader,
  useStorageLocationsLoader,
  useStudioItemsLoader,
} from '../../hooks/useServerData'
import { filterStudioItems, readStudioFilters, writeStudioFilters } from './studioListing'

// `/studio`: every item as a tile, filtered by project, live or deleted.
export function StudioPage() {
  const { t } = useTranslation('studio')
  const createState = useOverlayState()
  const [ searchParams, setSearchParams ] = useSearchParams()
  const filters = readStudioFilters(searchParams)

  const liveStatus = useStudioItemsLoader()
  const deletedStatus = useDeletedStudioItemsLoader(filters.deleted)
  const locationsStatus = useStorageLocationsLoader()
  // The New item form picks a satellite; loading them here has them ready when it opens.
  useSatellitesLoader()

  const liveItems = useAppSelector(selectLiveStudioItems)
  const deletedItems = useAppSelector(selectDeletedStudioItems)
  const locations = useAppSelector(selectAllStorageLocations)
  const latestSessions = useAppSelector(selectLatestSessionsByStudioItemId, shallowEqual)

  const status = filters.deleted
    ? deletedStatus
    : liveStatus
  const items = filters.deleted
    ? deletedItems
    : liveItems
  const listedItems = filterStudioItems(items, filters)
  // Items cannot outlive their location, so with none there is nothing to show but the way
  // to add one.
  const hasNoStorage = locationsStatus === 'loaded' && !locations.length

  function emptyMessage() {
    if (items.length) {
      return t('grid.noMatches')
    }
    if (filters.deleted) {
      return t('grid.emptyDeleted')
    }
    return t('grid.empty')
  }

  return <div className='container'>
    <div className='level relaxed items-start'>
      <div>
        <h1 className='text-3xl font-bold'>{
          t('title')
        }</h1>
        <p className='mt-1 max-w-2xl text-sm opacity-70'>{
          t('description')
        }</p>
      </div>
      {!hasNoStorage && <Button
        size='sm'
        className='shrink-0'
        isDisabled={locationsStatus !== 'loaded'}
        onPress={createState.open}
      >
        <LuPlus className='size-4' aria-hidden />
        <span>{t('add')}</span>
      </Button>}
    </div>

    {hasNoStorage && <StudioNoStorage />}

    {!hasNoStorage && <>
      <StudioFiltersBar
        filters={filters}
        onChange={(next) => setSearchParams(writeStudioFilters(next), { replace: true })}
      />

      {status === 'loading' && <div className='grid place-items-center py-16'>
        <Spinner />
      </div>}

      {status === 'failed' && <p className='py-10 text-center text-sm text-danger'>{
        t('grid.loadError')
      }</p>}

      {status === 'loaded' && !listedItems.length && <EmptyNotice>{
        emptyMessage()
      }</EmptyNotice>}

      {status === 'loaded' && listedItems.length > 0 && <ul
        aria-label={t('grid.label')}
        className='grid grid-cols-[repeat(auto-fill,minmax(14rem,1fr))] gap-4'
      >{
          listedItems.map((item) => <li key={item.id}>
            <StudioItemTile
              item={item}
              latestSession={latestSessions[item.id]}
            />
          </li>)
        }</ul>}
    </>}

    {createState.isOpen && <CreateStudioItemModal state={createState} />}
  </div>
}
