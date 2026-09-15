// Copyright © 2026 Jalapeno Labs

import type { UseOverlayStateReturn } from '@heroui/react'
import type { MailDomain } from '../../../api/routes/mailRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { mailDomainDeleted } from '../../../store/mailDomainsSlice'

// User interface
import { AlertDialog, Button, toast } from '@heroui/react'

// Utility
import { HTTPError } from 'ky'

// Misc
import { getUpstreamErrorMessage } from '../../../api/errors'
import { deleteMailDomain } from '../../../api/routes/mailRoutes'

type Props = {
  state: UseOverlayStateReturn
  domain: MailDomain | null
  // Mailboxes on the domain. The API refuses to remove a domain that has any.
  mailboxCount: number
}

export function RemoveMailDomainDialog(props: Props) {
  const { t } = useTranslation([ 'email', 'common' ])
  const dispatch = useAppDispatch()
  const [ isRemoving, setIsRemoving ] = useState(false)
  const name = props.domain?.name ?? ''

  async function onConfirm() {
    if (!props.domain) {
      console.debug('RemoveMailDomainDialog confirmed with no domain selected')
      return
    }

    setIsRemoving(true)
    try {
      await deleteMailDomain(props.domain.id)
      dispatch(mailDomainDeleted(props.domain.id))
      toast.success(t('toasts.domainRemoved', { name }))
      props.state.close()
    }
    catch (error) {
      // A 409 means a mailbox was added elsewhere since the dialog opened.
      const description = error instanceof HTTPError && error.response.status === 409
        ? t('removeDomain.hasMailboxes', { count: Math.max(props.mailboxCount, 1) })
        : getUpstreamErrorMessage(error) ?? t('common:errors.unexpected')
      console.debug('RemoveMailDomainDialog failed to remove the domain', { error })
      toast.danger(t('toasts.domainRemoveFailed', { name }), { description })
    }
    finally {
      setIsRemoving(false)
    }
  }

  return <AlertDialog.Backdrop isOpen={props.state.isOpen} onOpenChange={props.state.setOpen}>
    <AlertDialog.Container>
      <AlertDialog.Dialog className='sm:max-w-md'>
        <AlertDialog.Header>
          <AlertDialog.Icon status='danger' />
          <AlertDialog.Heading>{
            t('removeDomain.title', { name })
          }</AlertDialog.Heading>
        </AlertDialog.Header>
        <AlertDialog.Body>
          <p>{
            props.mailboxCount
              ? t('removeDomain.hasMailboxes', { count: props.mailboxCount })
              : t('removeDomain.description')
          }</p>
        </AlertDialog.Body>
        <AlertDialog.Footer>
          <Button slot='close' variant='tertiary'>
            <span>{t('common:actions.cancel')}</span>
          </Button>
          <Button
            variant='danger'
            isDisabled={props.mailboxCount > 0}
            isPending={isRemoving}
            onPress={onConfirm}
          >
            <span>{t('removeDomain.action')}</span>
          </Button>
        </AlertDialog.Footer>
      </AlertDialog.Dialog>
    </AlertDialog.Container>
  </AlertDialog.Backdrop>
}
