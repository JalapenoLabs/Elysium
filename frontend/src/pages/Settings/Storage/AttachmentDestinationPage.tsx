// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'
import useSWR from 'swr'

// Redux
import { selectIsAdmin } from '../../../store/authSlice'
import { useAppSelector } from '../../../store/hooks'
import { selectAllStorageLocations } from '../../../store/storageLocationsSlice'

// User interface
import { Spinner } from '@heroui/react'
import { EmptyNotice } from '../../../components/EmptyNotice'
import { SettingsPage } from '../SettingsPage'
import { AttachmentDestinationForm } from './AttachmentDestinationForm'

// Misc
import { getPageAttachmentPolicy, PAGE_ATTACHMENT_POLICY_KEY } from '../../../api/routes/pageRoutes'
import { useStorageLocationsLoader } from '../../../hooks/useServerData'
import { UrlTree } from '../../../urls'

// `/settings/storage/attachments`: the storage location page attachments go to, on a page of its own
// beside the list of locations. Admins only.
export function AttachmentDestinationPage() {
  const { t } = useTranslation('storage')
  const isAdmin = useAppSelector(selectIsAdmin)

  return <SettingsPage
    title={t('attachments.title')}
    parents={[{ label: t('title'), href: UrlTree.settingsStorage }]}
  >
    {isAdmin
      ? <DestinationForm />
      : <EmptyNotice>{t('attachments.adminOnly')}</EmptyNotice>}
  </SettingsPage>
}

// Loads the storage locations only for an admin, whom the API answers.
function DestinationForm() {
  const { t } = useTranslation('storage')
  const navigate = useNavigate()
  const { data, error } = useSWR(PAGE_ATTACHMENT_POLICY_KEY, getPageAttachmentPolicy)
  const loadLocations = useStorageLocationsLoader()
  const locations = useAppSelector(selectAllStorageLocations)

  if (data && loadLocations !== 'loading') {
    return <AttachmentDestinationForm
      destinationId={data.policy.destinationId}
      locations={locations}
      onSaved={() => void navigate(UrlTree.settingsStorage)}
    />
  }
  if (error || loadLocations === 'failed') {
    return <p className='py-10 text-center text-sm text-danger'>{t('attachments.loadError')}</p>
  }
  return <div className='grid place-items-center py-16'>
    <Spinner />
  </div>
}
