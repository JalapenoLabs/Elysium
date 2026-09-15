// Copyright © 2026 Jalapeno Labs

// Core
import { useNavigate, useParams } from 'react-router'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectStorageLocationById } from '../../../store/storageLocationsSlice'

// User interface
import { Link, Spinner } from '@heroui/react'
import { StorageLocationEditorLayout } from './StorageLocationEditorLayout'
import { StorageLocationForm } from './StorageLocationForm'

// Misc
import { useStorageLocationsLoader } from '../../../hooks/useServerData'
import { UrlTree } from '../../../urls'

// `/settings/storage/:locationId/edit`: editing one storage location.
export function EditStorageLocationPage() {
  const { t } = useTranslation('storage')
  const navigate = useNavigate()
  const { locationId = '' } = useParams()
  const status = useStorageLocationsLoader()
  const location = useAppSelector((state) => selectStorageLocationById(state, locationId))

  if (!location) {
    if (status === 'loading') {
      return <div className='grid place-items-center py-16'>
        <Spinner />
      </div>
    }
    return <div className='container py-10 text-center text-sm'>
      <p className='compact opacity-70'>{t('page.notFound')}</p>
      <Link href={UrlTree.settingsStorage} className='text-link'>{t('page.backToList')}</Link>
    </div>
  }

  return <StorageLocationEditorLayout
    title={t('form.editTitle', { name: location.name })}
  >
    <StorageLocationForm
      // Remount when navigating between locations so the form takes the new values.
      key={location.id}
      location={location}
      onSaved={() => navigate(UrlTree.settingsStorage)}
      onCancel={() => navigate(UrlTree.settingsStorage)}
    />
  </StorageLocationEditorLayout>
}
