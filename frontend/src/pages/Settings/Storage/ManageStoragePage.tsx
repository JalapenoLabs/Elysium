// Copyright © 2026 Jalapeno Labs

import type { StorageLocation } from '../../../api/routes/storageRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import { selectAllStorageLocations, storageLocationDeleted } from '../../../store/storageLocationsSlice'

// User interface
import { Breadcrumbs, Button, Spinner, toast, useOverlayState } from '@heroui/react'
import { LuPlus } from 'react-icons/lu'
import { StorageLocationFormModal } from './StorageLocationFormModal'
import { StorageLocationTable } from './StorageLocationTable'

// Misc
import { getUpstreamErrorMessage } from '../../../api/errors'
import { deleteStorageLocation, testStorageLocation } from '../../../api/routes/storageRoutes'
import { useConfirm } from '../../../hooks/useConfirm'
import { useStorageLocationsLoader } from '../../../hooks/useServerData'
import { UrlTree } from '../../../urls'

// `/settings/storage`: the external locations Elysium saves files to.
export function ManageStoragePage() {
  const { t } = useTranslation([ 'storage', 'settings', 'common' ])
  const dispatch = useAppDispatch()
  const confirm = useConfirm()
  const locations = useAppSelector(selectAllStorageLocations)
  const status = useStorageLocationsLoader()

  const formState = useOverlayState()
  const [ selectedLocation, setSelectedLocation ] = useState<StorageLocation | null>(null)
  // Remounting the form per opening resets it to the chosen location's values.
  const [ formSession, setFormSession ] = useState(0)

  function openForm(location: StorageLocation | null) {
    setSelectedLocation(location)
    setFormSession((session) => session + 1)
    formState.open()
  }

  async function runConnectionTest(location: StorageLocation) {
    try {
      const { result } = await testStorageLocation(location.id)
      toast.success(t('toasts.testPassed', { name: location.name, count: result.entries }))
    }
    catch (error) {
      const message = getUpstreamErrorMessage(error)
      if (!message) {
        console.debug('ManageStoragePage failed to test a location', { error, locationId: location.id })
      }
      toast.danger(t('toasts.testFailed', { name: location.name }), {
        description: message ?? t('common:errors.unexpected'),
      })
    }
  }

  function confirmDelete(location: StorageLocation) {
    confirm({
      title: t('delete.title', { name: location.name }),
      message: t('delete.body'),
      tone: 'danger',
      confirmText: t('common:actions.delete'),
      onConfirm: async () => {
        try {
          await deleteStorageLocation(location.id)
        }
        catch (error) {
          console.debug('ManageStoragePage failed to delete a location', { error, locationId: location.id })
          toast.danger(t('common:errors.unexpected'))
          // Keeps the dialog open to try again.
          throw error
        }

        dispatch(storageLocationDeleted(location.id))
        toast.success(t('toasts.deleted', { name: location.name }))
      },
    })
  }

  return <div className='container'>
    <Breadcrumbs className='compact'>
      <Breadcrumbs.Item href={UrlTree.settings}>{t('settings:title')}</Breadcrumbs.Item>
      <Breadcrumbs.Item>{t('title')}</Breadcrumbs.Item>
    </Breadcrumbs>
    <h1 className='relaxed text-3xl font-bold'>{
      t('title')
    }</h1>

    <section>
      <div className='level compact items-start'>
        <div>
          <h2 className='text-xl font-semibold'>{
            t('locations.heading')
          }</h2>
          <p className='mt-1 max-w-2xl text-sm opacity-70'>{
            t('locations.description')
          }</p>
        </div>
        <Button
          size='sm'
          variant='outline'
          className='shrink-0'
          onPress={() => openForm(null)}
        >
          <LuPlus className='size-4' aria-hidden />
          <span>{t('locations.add')}</span>
        </Button>
      </div>

      {status === 'loading' && <div className='grid place-items-center py-16'>
        <Spinner />
      </div>}

      {status === 'failed' && <p className='py-10 text-center text-sm text-danger'>{
        t('table.loadError')
      }</p>}

      {status === 'loaded' && <StorageLocationTable
        locations={locations}
        onEdit={(location) => openForm(location)}
        onTest={runConnectionTest}
        onDelete={confirmDelete}
      />}
    </section>

    <StorageLocationFormModal
      key={formSession}
      state={formState}
      location={selectedLocation}
    />
  </div>
}
