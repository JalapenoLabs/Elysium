// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectAllStorageLocations } from '../../store/storageLocationsSlice'

// User interface
import { Spinner } from '@heroui/react'
import { Page } from '../../layout/page/Page'
import { CreateStudioItemForm } from './CreateStudioItemForm'
import { StudioNoStorage } from './StudioNoStorage'

// Misc
import { useStorageLocationsLoader } from '../../hooks/useServerData'
import { UrlTree } from '../../urls'

// `/studio/new`: starting a Studio item. The form opens once the storage locations are loaded,
// since it picks its default location from them as it mounts; with none, an item has nowhere to
// keep its files, so the page offers the way to add one instead.
export function NewStudioItemPage() {
  const { t } = useTranslation('studio')
  const status = useStorageLocationsLoader()
  const locations = useAppSelector(selectAllStorageLocations)

  return <Page
    title={t('create.title')}
    parents={[{ label: t('title'), href: UrlTree.studio }]}
  >
    {status === 'loading' && <div className='grid place-items-center py-16'>
      <Spinner />
    </div>}

    {status === 'failed' && <p className='py-10 text-center text-sm text-danger'>{
      t('create.loadError')
    }</p>}

    {status === 'loaded' && !locations.length && <StudioNoStorage />}

    {status === 'loaded' && locations.length > 0 && <CreateStudioItemForm />}
  </Page>
}
