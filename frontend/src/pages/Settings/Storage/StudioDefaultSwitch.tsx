// Copyright © 2026 Jalapeno Labs

import type { StorageLocation } from '../../../api/routes/storageRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { storageLocationUpserted } from '../../../store/storageLocationsSlice'

// User interface
import { Label, Switch, toast } from '@heroui/react'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { updateStorageLocation } from '../../../api/routes/storageRoutes'

type Props = {
  location: StorageLocation
}

// Marks the location Studio's New item form saves to by default. At most one holds it, so
// switching one on moves it from any other, which the slice mirrors at once.
export function StudioDefaultSwitch(props: Props) {
  const { t } = useTranslation([ 'storage', 'common' ])
  const dispatch = useAppDispatch()
  const [ isSaving, setIsSaving ] = useState(false)
  const location = props.location

  async function change(isStudioDefault: boolean) {
    setIsSaving(true)
    try {
      const response = await updateStorageLocation(location.id, { isStudioDefault })
      dispatch(storageLocationUpserted(response.location))
      toast.success(isStudioDefault
        ? t('toasts.studioDefaultSet', { name: location.name })
        : t('toasts.studioDefaultCleared', { name: location.name }))
    }
    catch (error) {
      const message = getApiErrorMessage(error)
      if (!message) {
        console.debug('StudioDefaultSwitch failed to change the default', { error, locationId: location.id })
      }
      toast.danger(t('toasts.studioDefaultFailed'), {
        description: message ?? t('common:errors.unexpected'),
      })
    }
    finally {
      setIsSaving(false)
    }
  }

  return <Switch
    size='sm'
    isSelected={location.isStudioDefault}
    isDisabled={isSaving}
    onChange={(isSelected) => void change(isSelected)}
  >
    <Switch.Control>
      <Switch.Thumb />
    </Switch.Control>
    <Switch.Content>
      <Label className='sr-only'>{t('table.studioDefaultToggle', { name: location.name })}</Label>
    </Switch.Content>
  </Switch>
}
