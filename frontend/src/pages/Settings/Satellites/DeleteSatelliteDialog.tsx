// Copyright © 2026 Jalapeno Labs

import type { UseOverlayStateReturn } from '@heroui/react'
import type { Satellite } from '../../../api/routes/satelliteRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { satelliteDeleted } from '../../../store/satellitesSlice'

// User interface
import { AlertDialog, Button, toast } from '@heroui/react'

// Misc
import { deleteSatellite } from '../../../api/routes/satelliteRoutes'

type Props = {
  state: UseOverlayStateReturn
  satellite: Satellite | null
}

export function DeleteSatelliteDialog(props: Props) {
  const { t } = useTranslation([ 'satellites', 'common' ])
  const dispatch = useAppDispatch()
  const [ isDeleting, setIsDeleting ] = useState(false)

  async function onConfirm() {
    if (!props.satellite) {
      console.debug('DeleteSatelliteDialog confirmed with no satellite selected')
      return
    }

    setIsDeleting(true)
    try {
      await deleteSatellite(props.satellite.id)
      dispatch(satelliteDeleted(props.satellite.id))
      toast.success(t('toasts.deleted', { name: props.satellite.name }))
      props.state.close()
    }
    catch (error) {
      console.debug('DeleteSatelliteDialog failed to delete the satellite', { error })
      toast.danger(t('common:errors.unexpected'))
    }
    finally {
      setIsDeleting(false)
    }
  }

  // Controlled overlays skip the AlertDialog root: it is a trigger wrapper, and without
  // a pressable child React Aria warns. The backdrop takes the open state directly.
  return <AlertDialog.Backdrop isOpen={props.state.isOpen} onOpenChange={props.state.setOpen}>
    <AlertDialog.Container>
      <AlertDialog.Dialog className='sm:max-w-md'>
        <AlertDialog.Header>
          <AlertDialog.Icon status='danger' />
          <AlertDialog.Heading>{
            t('delete.title', { name: props.satellite?.name ?? '' })
          }</AlertDialog.Heading>
        </AlertDialog.Header>
        <AlertDialog.Body>
          <p>{
            t('delete.body')
          }</p>
        </AlertDialog.Body>
        <AlertDialog.Footer>
          <Button slot='close' variant='tertiary'>
            <span>{t('common:actions.cancel')}</span>
          </Button>
          <Button
            variant='danger'
            isPending={isDeleting}
            onPress={onConfirm}
          >
            <span>{t('common:actions.delete')}</span>
          </Button>
        </AlertDialog.Footer>
      </AlertDialog.Dialog>
    </AlertDialog.Container>
  </AlertDialog.Backdrop>
}
