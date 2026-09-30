// Copyright © 2026 Jalapeno Labs

import type { UseOverlayStateReturn } from '@heroui/react'
import type { User } from '../../../api/routes/userRoutes'

// Core
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Alert, Button, Modal, Spinner, toast } from '@heroui/react'
import { LuCopy } from 'react-icons/lu'

// Misc
import { createUserRecoveryLink } from '../../../api/routes/userRoutes'

type Props = {
  state: UseOverlayStateReturn
  // The person the link is for. The modal is mounted anew for each link, which makes it.
  user: User
}

type LinkState =
  | { kind: 'loading' }
  | { kind: 'ready', link: string, expiresAt: string }
  | { kind: 'failed' }

const timeFormatter = new Intl.DateTimeFormat(undefined, { timeStyle: 'short' })

// A one-time link an admin hands to someone locked out: it signs them in and opens the page
// to set a new password. Elysium keeps no copy, so it is shown once, here.
export function RecoveryLinkModal(props: Props) {
  const { t } = useTranslation([ 'users', 'common' ])
  const [ link, setLink ] = useState<LinkState>({ kind: 'loading' })
  const userId = props.user.id

  useEffect(() => {
    let isCurrent = true
    createUserRecoveryLink(userId)
      .then((response) => {
        if (isCurrent) {
          setLink({ kind: 'ready', link: response.recoveryLink, expiresAt: response.expiresAt })
        }
      })
      .catch((error: unknown) => {
        console.debug('RecoveryLinkModal could not create a recovery link', { error, userId })
        if (isCurrent) {
          setLink({ kind: 'failed' })
        }
      })
    return () => {
      isCurrent = false
    }
  }, [ userId ])

  async function copy(value: string) {
    try {
      await navigator.clipboard.writeText(value)
      toast.success(t('recoveryLink.copied'))
    }
    catch (error) {
      console.debug('RecoveryLinkModal could not copy the link', { error })
      toast.danger(t('recoveryLink.copyFailed'))
    }
  }

  return <Modal.Backdrop isOpen={props.state.isOpen} onOpenChange={props.state.setOpen}>
    <Modal.Container>
      <Modal.Dialog className='sm:max-w-lg'>
        <Modal.CloseTrigger />
        <Modal.Header>
          <Modal.Heading>{t('recoveryLink.title', { name: props.user.name })}</Modal.Heading>
        </Modal.Header>
        <Modal.Body className='mt-2'>
          {link.kind === 'loading' && <div className='grid place-items-center py-6'>
            <Spinner />
          </div>}
          {link.kind === 'failed' && <p className='text-sm text-danger'>{t('recoveryLink.failed')}</p>}
          {link.kind === 'ready' && <>
            <p className='compact text-sm opacity-80'>{t('recoveryLink.description')}</p>
            <div className='compact flex items-start gap-2'>
              <code className='min-w-0 flex-1 break-all rounded-md bg-surface-secondary px-3 py-2 text-xs'>{
                link.link
              }</code>
              <Button
                isIconOnly
                size='sm'
                variant='outline'
                aria-label={t('recoveryLink.copy')}
                onPress={() => copy(link.link)}
              >
                <LuCopy className='size-4' aria-hidden />
              </Button>
            </div>
            <Alert status='warning'>
              <Alert.Indicator />
              <Alert.Content>
                <Alert.Description>{
                  t('recoveryLink.expires', { time: timeFormatter.format(new Date(link.expiresAt)) })
                }</Alert.Description>
              </Alert.Content>
            </Alert>
          </>}
        </Modal.Body>
        <Modal.Footer>
          <Button slot='close' variant='tertiary'>
            <span>{t('common:actions.close')}</span>
          </Button>
        </Modal.Footer>
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>
}
