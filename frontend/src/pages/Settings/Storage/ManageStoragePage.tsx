// Copyright © 2026 Jalapeno Labs

import type { StorageLocation } from '../../../api/routes/storageRoutes'

// Core
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// Redux
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import { selectAllStorageLocations, storageLocationDeleted } from '../../../store/storageLocationsSlice'

// User interface
import { Breadcrumbs, Button, Spinner, toast } from '@heroui/react'
import { LuPlus } from 'react-icons/lu'
import { StorageLocationTable } from './StorageLocationTable'

// Misc
import { getUpstreamErrorMessage } from '../../../api/errors'
import { deleteStorageLocation, testStorageLocation } from '../../../api/routes/storageRoutes'
import { useConfirm } from '../../../hooks/useConfirm'
import { useStorageLocationsLoader } from '../../../hooks/useServerData'
import { getStorageLocationEditUrl, UrlTree } from '../../../urls'

// `/settings/storage`: the external locations Elysium saves files to.
export function ManageStoragePage() {
  const { t } = useTranslation([ 'storage', 'settings', 'common' ])
  const dispatch = useAppDispatch()
  const confirm = useConfirm()
  const locations = useAppSelector(selectAllStorageLocations)
  const status = useStorageLocationsLoader()
  const navigate = useNavigate()

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
    <section>
      <div className='level relaxed items-center'>
        <h1 className='text-3xl font-bold'>{
          t('title')
        }</h1>
        <Button
          size='sm'
          variant='outline'
          className='shrink-0'
          onPress={() => navigate(UrlTree.settingsStorageNew)}
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
        onEdit={(location) => navigate(getStorageLocationEditUrl(location.id))}
        onTest={runConnectionTest}
        onDelete={confirmDelete}
      />}
    </section>
  </div>
}
