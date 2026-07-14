// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// UI
import { Button, Input, Modal, ModalBody, ModalContent, ModalFooter, ModalHeader } from '@heroui/react'

// Misc
import { mintInvite } from '../../api/routes/authRoutes'

type Props = {
  isOpen: boolean
  onOpenChange: (isOpen: boolean) => void
}

// Mints a copyable invite link. The link itself is the secret (the API stores
// only its hash), so it is shown exactly once here: minting is an explicit
// action, never a side effect of opening the modal. Role and team targeting
// join once finer-grained invite authorization ships.
export function InvitePeopleModal(props: Props) {
  const { t } = useTranslation('common')

  const [ inviteUrl, setInviteUrl ] = useState<string | null>(null)
  const [ isMinting, setIsMinting ] = useState(false)
  const [ hasCopied, setHasCopied ] = useState(false)
  const [ errorMessage, setErrorMessage ] = useState<string | null>(null)

  async function onMint() {
    setIsMinting(true)
    setErrorMessage(null)
    try {
      const invite = await mintInvite()
      setInviteUrl(invite.url)
      setHasCopied(false)
    }
    catch (error) {
      console.debug('invite mint failed', { error })
      setErrorMessage(t('invite.failed'))
    }
    finally {
      setIsMinting(false)
    }
  }

  async function onCopy() {
    if (!inviteUrl) {
      console.debug('copy requested before an invite link was minted')
      return
    }
    try {
      await navigator.clipboard.writeText(inviteUrl)
      setHasCopied(true)
    }
    catch (error) {
      console.debug('clipboard write failed', { error })
    }
  }

  // Each open starts fresh so a stale link never lingers on screen.
  function onOpenChange(isOpen: boolean) {
    if (!isOpen) {
      setInviteUrl(null)
      setHasCopied(false)
      setErrorMessage(null)
    }
    props.onOpenChange(isOpen)
  }

  return <Modal isOpen={props.isOpen} onOpenChange={onOpenChange}>
    <ModalContent>
      <ModalHeader className='flex-col items-start gap-1'>
        <span>{ t('invite.title') }</span>
        <span className='text-sm font-normal opacity-70'>{ t('invite.subtitle') }</span>
      </ModalHeader>
      <ModalBody>
        { inviteUrl
          ? <div>
            <div className='compact'>
              <Input
                isReadOnly
                label={t('invite.linkLabel')}
                value={inviteUrl}
                description={t('invite.linkHint')}
              />
            </div>
            <Button
              color='primary'
              variant='flat'
              className='w-full'
              onPress={onCopy}
            >
              <span>{ hasCopied ? t('actions.copied') : t('actions.copy') }</span>
            </Button>
          </div>
          : <p className='opacity-80'>{ t('invite.explainer') }</p>
        }

        { errorMessage
          ? <p className='text-sm text-danger'>{ errorMessage }</p>
          : null }
      </ModalBody>
      <ModalFooter>
        <Button variant='light' onPress={() => onOpenChange(false)}>
          <span>{ t('actions.done') }</span>
        </Button>
        <Button
          color='primary'
          isLoading={isMinting}
          isDisabled={isMinting}
          onPress={onMint}
        >
          <span>{ inviteUrl ? t('invite.mintAnother') : t('invite.mint') }</span>
        </Button>
      </ModalFooter>
    </ModalContent>
  </Modal>
}
