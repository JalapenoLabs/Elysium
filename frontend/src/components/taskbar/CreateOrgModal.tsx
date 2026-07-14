// Copyright © 2026 Jalapeno Labs

import type { FormEvent } from 'react'

// Core
import { useState } from 'react'
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// UI
import { Button, Input, Modal, ModalBody, ModalContent, ModalFooter, ModalHeader } from '@heroui/react'

// Misc
import { createOrg } from '../../api/routes/orgRoutes'
import { resolveSession } from '../../auth/session'
import { UrlTree } from '../../urls'

type Props = {
  isOpen: boolean
  onOpenChange: (isOpen: boolean) => void
}

// Creates a new organization. The server makes the creator its owner and flips
// the session's active org to it, so on success we land home in the new org's
// context.
export function CreateOrgModal(props: Props) {
  const { t } = useTranslation('common')
  const navigate = useNavigate()

  const [ name, setName ] = useState('')
  const [ isCreating, setIsCreating ] = useState(false)
  const [ errorMessage, setErrorMessage ] = useState<string | null>(null)

  async function onCreate(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!name.trim()) {
      return
    }

    setIsCreating(true)
    setErrorMessage(null)
    try {
      await createOrg(name.trim())
      // The server switched the active org to the new one; reseed the session
      // context so the taskbar reflects it immediately.
      await resolveSession()
      props.onOpenChange(false)
      setName('')
      navigate(UrlTree.home)
    }
    catch (error) {
      console.debug('org creation failed', { error })
      setErrorMessage(t('org.createFailed'))
    }
    finally {
      setIsCreating(false)
    }
  }

  return <Modal isOpen={props.isOpen} onOpenChange={props.onOpenChange}>
    <ModalContent>
      <form onSubmit={onCreate}>
        <ModalHeader>{ t('org.createTitle') }</ModalHeader>
        <ModalBody>
          <Input
            autoFocus
            label={t('org.nameLabel')}
            value={name}
            onChange={(event) => setName(event.currentTarget.value)}
          />
          { errorMessage
            ? <p className='text-sm text-danger'>{ errorMessage }</p>
            : null }
        </ModalBody>
        <ModalFooter>
          <Button variant='light' onPress={() => props.onOpenChange(false)}>
            <span>{ t('actions.cancel') }</span>
          </Button>
          <Button
            type='submit'
            color='primary'
            isLoading={isCreating}
            isDisabled={isCreating || !name.trim()}
          >
            <span>{ t('org.createAction') }</span>
          </Button>
        </ModalFooter>
      </form>
    </ModalContent>
  </Modal>
}
