// Copyright © 2026 Jalapeno Labs

// Core
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// User interface
import { StorageLocationEditorLayout } from './StorageLocationEditorLayout'
import { StorageLocationForm } from './StorageLocationForm'

// Misc
import { UrlTree } from '../../../urls'

// `/settings/storage/new`: adding a storage location. Bunny Storage is the only provider
// today, so its steps sit beside the form.
export function AddStorageLocationPage() {
  const { t } = useTranslation('storage')
  const navigate = useNavigate()

  return <StorageLocationEditorLayout
    title={t('form.createTitle')}
    kind='bunny'
  >
    <StorageLocationForm
      location={null}
      onSaved={() => navigate(UrlTree.settingsStorage)}
      onCancel={() => navigate(UrlTree.settingsStorage)}
    />
  </StorageLocationEditorLayout>
}
