// Copyright © 2026 Jalapeno Labs

import type { FormEvent } from 'react'
import type { StorageLocation } from '../../../api/routes/storageRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { backupsStatusUpdated } from '../../../store/backupsSlice'
import { useAppDispatch } from '../../../store/hooks'

// User interface
import { Form, Link, toast } from '@heroui/react'
import { OptionSelect } from '../../../components/OptionSelect'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { chooseBackupDestination } from '../../../api/routes/backupRoutes'
import { UrlTree } from '../../../urls'
import { usePageSave } from '../../../layout/page/pageSave'

const BACKUP_DESTINATION_FORM_ID = 'backup-destination-form'

// The select's id for backing up nowhere. Location ids are UUIDs, so it never collides.
const NOWHERE = 'nowhere'

type Props = {
  // The location backups go to now, or null for none.
  destinationId: string | null
  locations: StorageLocation[]
}

// Where the databases are backed up to, saved from the page's Save in the header. The API
// reaches the location before it is chosen, so a wrong key is reported here rather than at
// night.
export function BackupDestinationForm(props: Props) {
  const { t } = useTranslation([ 'backups', 'common' ])
  const dispatch = useAppDispatch()
  const saved = props.destinationId ?? NOWHERE
  const [ chosen, setChosen ] = useState(saved)
  const [ isSaving, setIsSaving ] = useState(false)

  usePageSave({
    formId: BACKUP_DESTINATION_FORM_ID,
    isDirty: chosen !== saved,
    isPending: isSaving,
  })

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setIsSaving(true)
    try {
      const storageLocationId = chosen === NOWHERE
        ? null
        : chosen
      const response = await chooseBackupDestination(storageLocationId)
      dispatch(backupsStatusUpdated(response.backups))
      const destination = response.backups.destination
      toast.success(destination
        ? t('destination.saved', { name: destination.name })
        : t('destination.turnedOff'))
    }
    catch (error) {
      const message = getApiErrorMessage(error)
      if (!message) {
        console.debug('BackupDestinationForm could not choose a destination', { error, chosen })
      }
      toast.danger(t('destination.failed'), {
        description: message ?? t('common:errors.unexpected'),
      })
    }
    setIsSaving(false)
  }

  const options = [
    { id: NOWHERE, label: t('destination.none') },
    ...props.locations.map((location) => ({ id: location.id, label: location.name })),
  ]

  return <Form
    id={BACKUP_DESTINATION_FORM_ID}
    className='relaxed max-w-md'
    onSubmit={save}
  >
    <OptionSelect
      label={t('destination.label')}
      options={options}
      value={chosen}
      onChange={setChosen}
    />
    {!props.locations.length && <Link
      className='mt-2 text-sm'
      href={UrlTree.settingsStorageNew}
    >{
        t('destination.addLocation')
      }</Link>}
  </Form>
}
