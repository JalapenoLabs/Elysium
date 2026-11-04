// Copyright © 2026 Jalapeno Labs

import type { StorageLocation } from '../../../api/routes/storageRoutes'

// Core
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// Redux
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import { selectAllStorageLocations, storageLocationDeleted } from '../../../store/storageLocationsSlice'

// User interface
import { Button, Spinner } from '@heroui/react'
import { LuPlus } from 'react-icons/lu'
import { SettingsPage } from '../SettingsPage'
import { StorageLocationTable } from './StorageLocationTable'

// Misc
import { getApiErrorCode, getApiErrorMessage, getUpstreamErrorMessage } from '../../../api/errors'
import { deleteStorageLocation, testStorageLocation } from '../../../api/routes/storageRoutes'
import { useConfirm } from '../../../hooks/useConfirm'
import { useStorageLocationsLoader } from '../../../hooks/useServerData'
import { getStorageLocationEditUrl, UrlTree } from '../../../urls'
import { toast } from '../../../toast/toast'

// `/settings/storage`: the external locations Elysium saves files to.
export function ManageStoragePage() {
  const { t } = useTranslation([ 'storage', 'common' ])
  const dispatch = useAppDispatch()
  const confirm = useConfirm()
  const locations = useAppSelector(selectAllStorageLocations)
  const status = useStorageLocationsLoader()
  const navigate = useNavigate()

  async function runConnectionTest(location: StorageLocation) {
    try {
      const { result } = await testStorageLocation(location.id)
      const key = result.hasMore
        ? 'toasts.testPassedMore'
        : 'toasts.testPassed'
      toast.success(t(key, { name: location.name, count: result.entries }))
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
      message: t('delete.body', { name: location.name }),
      isIrreversible: true,
      confirmText: t('common:actions.delete'),
      onConfirm: async () => {
        try {
          await deleteStorageLocation(location.id)
        }
        catch (error) {
          console.debug('ManageStoragePage failed to delete a location', { error, locationId: location.id })
          // The API's own words for a refusal, such as Studio items that keep files here.
          const message = getApiErrorCode(error) === 'backup_destination'
            ? t('delete.backupDestination')
            : getApiErrorMessage(error)
          toast.danger(message ?? t('common:errors.unexpected'))
          // Keeps the dialog open to try again.
          throw error
        }

        dispatch(storageLocationDeleted(location.id))
        toast.success(t('toasts.deleted', { name: location.name }))
      },
    })
  }

  return <SettingsPage
    title={t('title')}
    primaryAction={<Button
      variant='outline'
      onPress={() => navigate(UrlTree.settingsStorageNew)}
    >
      <LuPlus className='size-4' aria-hidden />
      <span>{t('locations.add')}</span>
    </Button>}
  >
    <section>

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
  </SettingsPage>
}
