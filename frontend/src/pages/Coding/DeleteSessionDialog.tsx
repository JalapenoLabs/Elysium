// Copyright © 2026 Jalapeno Labs

import type { UseOverlayStateReturn } from '@heroui/react'
import type { CodingSession } from '../../api/routes/codingSessionRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { codingSessionDeleted } from '../../store/codingSessionsSlice'
import { useAppDispatch } from '../../store/hooks'

// User interface
import { AlertDialog, Button, toast } from '@heroui/react'

// Misc
import { getUpstreamErrorMessage } from '../../api/errors'
import { deleteCodingSession } from '../../api/routes/codingSessionRoutes'

type Props = {
  state: UseOverlayStateReturn
  session: CodingSession | null
}

export function DeleteSessionDialog(props: Props) {
  const { t } = useTranslation([ 'coding', 'common' ])
  const dispatch = useAppDispatch()
  const [ isDeleting, setIsDeleting ] = useState(false)

  async function onConfirm() {
    if (!props.session) {
      console.debug('DeleteSessionDialog confirmed with no session selected')
      return
    }

    setIsDeleting(true)
    try {
      await deleteCodingSession(props.session.id)
      dispatch(codingSessionDeleted(props.session.id))
      toast.success(t('toasts.deleted', { title: props.session.title }))
      props.state.close()
    }
    catch (error) {
      const message = getUpstreamErrorMessage(error)
      if (!message) {
        console.debug('DeleteSessionDialog failed to delete the session', { error })
      }
      toast.danger(message ?? t('common:errors.unexpected'))
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
            t('delete.title', { title: props.session?.title ?? '' })
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
