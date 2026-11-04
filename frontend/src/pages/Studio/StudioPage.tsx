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
import { Page } from '../../layout/page/Page'
import { EmptyNotice } from '../../components/EmptyNotice'
import { CreateStudioItemModal } from './CreateStudioItemModal'
import { StudioFiltersBar } from './StudioFiltersBar'
import { StudioItemTile } from './StudioItemTile'
import { StudioNoStorage } from './StudioNoStorage'

// Misc
import {
  useDeletedStudioItemsLoader,
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

  return <Page
    title={t('title')}
    primaryAction={!hasNoStorage && <Button
      isDisabled={locationsStatus !== 'loaded'}
      onPress={createState.open}
    >
      <LuPlus className='size-4' aria-hidden />
      <span>{t('add')}</span>
    </Button>}
  >

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
  </Page>
}
