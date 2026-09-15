// Copyright © 2026 Jalapeno Labs

import type { UseOverlayStateReturn } from '@heroui/react'
import type { MailAccount } from '../../../api/routes/mailRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { mailAccountDeleted } from '../../../store/mailAccountsSlice'

// User interface
import { AlertDialog, Button, toast } from '@heroui/react'

// Misc
import { getUpstreamErrorMessage } from '../../../api/errors'
import { deleteMailAccount } from '../../../api/routes/mailRoutes'
import { mailAccountKindLabelKeys } from './mailPresentation'

type Props = {
  state: UseOverlayStateReturn
  account: MailAccount | null
}

// Disconnecting means different things per kind: a self-hosted mailbox is destroyed
// with its mail, an OAuth account is only forgotten. The dialog says which.
export function DisconnectMailAccountDialog(props: Props) {
  const { t } = useTranslation([ 'email', 'common' ])
  const dispatch = useAppDispatch()
  const [ isDeleting, setIsDeleting ] = useState(false)

  const isSelfHosted = props.account?.kind === 'self-hosted'

  async function onConfirm() {
    if (!props.account) {
      console.debug('DisconnectMailAccountDialog confirmed with no account selected')
      return
    }

    setIsDeleting(true)
    try {
      await deleteMailAccount(props.account.id)
      dispatch(mailAccountDeleted(props.account.id))
      toast.success(t('toasts.disconnected', { address: props.account.address }))
      props.state.close()
    }
    catch (error) {
      const message = getUpstreamErrorMessage(error)
      if (!message) {
        console.debug('DisconnectMailAccountDialog failed to disconnect the account', { error })
      }
      toast.danger(message ?? t('common:errors.unexpected'))
    }
    finally {
      setIsDeleting(false)
    }
  }

  return <AlertDialog.Backdrop isOpen={props.state.isOpen} onOpenChange={props.state.setOpen}>
    <AlertDialog.Container>
      <AlertDialog.Dialog className='sm:max-w-md'>
        <AlertDialog.Header>
          <AlertDialog.Icon status='danger' />
          <AlertDialog.Heading>{
            isSelfHosted
              ? t('disconnect.deleteTitle', { address: props.account?.address ?? '' })
              : t('disconnect.title', { address: props.account?.address ?? '' })
          }</AlertDialog.Heading>
        </AlertDialog.Header>
        <AlertDialog.Body>
          <p>{
            isSelfHosted
              ? t('disconnect.selfHosted')
              : t('disconnect.oauth', {
                provider: props.account
                  ? t(mailAccountKindLabelKeys[props.account.kind])
                  : '',
              })
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
            <span>{
              isSelfHosted
                ? t('actions.deleteMailbox')
                : t('actions.disconnect')
            }</span>
          </Button>
        </AlertDialog.Footer>
      </AlertDialog.Dialog>
    </AlertDialog.Container>
  </AlertDialog.Backdrop>
}
