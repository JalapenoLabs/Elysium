// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

// UI
import {
  Button,
  Input,
  Modal,
  ModalBody,
  ModalContent,
  ModalFooter,
  ModalHeader,
  Tab,
  Tabs,
  Textarea,
} from '@heroui/react'
import { OneTimeSecretCard } from '../../../components/enrollment/OneTimeSecretCard'

// Misc
import { registerSshKey } from '../../../api/routes/userRoutes'

// A generated ed25519 private key downloads under this filename. A filename is
// an identifier, so it stays English regardless of locale.
const SSH_PRIVATE_KEY_FILENAME = 'elysium-ssh-key'

type Props = {
  isOpen: boolean
  onClose: () => void
  onCreated: () => void
}

// Adds an SSH key: either register the user's own public key, or have Elysium
// generate an ed25519 keypair and reveal the private key exactly once. The
// modal transitions collect -> reveal in place; the reveal only appears on the
// generate path (registering an existing key returns no private key).
export function SshKeyModal(props: Props) {
  const { t } = useTranslation([ 'auth', 'common' ])

  const [ mode, setMode ] = useState<'generate' | 'existing'>('generate')
  const [ name, setName ] = useState('')
  const [ publicKey, setPublicKey ] = useState('')
  const [ isBusy, setIsBusy ] = useState(false)
  const [ errorMessage, setErrorMessage ] = useState<string | null>(null)
  const [ revealedPrivateKey, setRevealedPrivateKey ] = useState<string | null>(null)

  useEffect(() => {
    if (props.isOpen) {
      setMode('generate')
      setName('')
      setPublicKey('')
      setErrorMessage(null)
      setRevealedPrivateKey(null)
    }
  }, [ props.isOpen ])

  const canSubmit = Boolean(name.trim())
    && (mode === 'generate' || Boolean(publicKey.trim()))
    && !isBusy

  async function onSubmit() {
    setIsBusy(true)
    setErrorMessage(null)
    try {
      const created = await registerSshKey(
        name.trim(),
        mode === 'existing' ? publicKey.trim() : '',
      )
      props.onCreated()
      if (created.privateKey) {
        setRevealedPrivateKey(created.privateKey)
      }
      else {
        props.onClose()
      }
    }
    catch (error) {
      console.debug('ssh key registration failed', { error })
      setErrorMessage(t('auth:settings.sshKeys.saveFailed'))
    }
    finally {
      setIsBusy(false)
    }
  }

  return <Modal isOpen={props.isOpen} onClose={props.onClose} size='lg' scrollBehavior='inside'>
    <ModalContent>
      { revealedPrivateKey
        ? <ModalBody className='py-6'>
          <OneTimeSecretCard
            title={t('auth:settings.sshKeys.revealTitle')}
            subtitle={t('auth:settings.sshKeys.revealSubtitle')}
            secret={revealedPrivateKey}
            copyLabel={t('common:actions.copy')}
            downloadLabel={t('auth:settings.sshKeys.download')}
            downloadFilename={SSH_PRIVATE_KEY_FILENAME}
            actionLabel={t('auth:settings.sshKeys.revealDone')}
            onDone={props.onClose}
          />
        </ModalBody>
        : <>
          <ModalHeader>{ t('auth:settings.sshKeys.addTitle') }</ModalHeader>
          <ModalBody>
            <Tabs
              selectedKey={mode}
              onSelectionChange={(key) => setMode(key as 'generate' | 'existing')}
            >
              <Tab key='generate' title={t('auth:settings.sshKeys.generateTab')} />
              <Tab key='existing' title={t('auth:settings.sshKeys.existingTab')} />
            </Tabs>
            <div className='compact'>
              <Input
                autoFocus
                label={t('auth:settings.sshKeys.nameLabel')}
                value={name}
                isDisabled={isBusy}
                onValueChange={setName}
              />
            </div>
            { mode === 'generate'
              ? <p className='text-sm opacity-70'>{ t('auth:settings.sshKeys.generateHint') }</p>
              : <Textarea
                label={t('auth:settings.sshKeys.publicKeyLabel')}
                placeholder='ssh-ed25519 AAAA...'
                minRows={3}
                value={publicKey}
                isDisabled={isBusy}
                onValueChange={setPublicKey}
              /> }
            { errorMessage
              ? <p className='text-sm text-danger'>{ errorMessage }</p>
              : null }
          </ModalBody>
          <ModalFooter>
            <Button variant='light' isDisabled={isBusy} onPress={props.onClose}>
              <span>{ t('common:actions.cancel') }</span>
            </Button>
            <Button color='primary' isDisabled={!canSubmit} isLoading={isBusy} onPress={onSubmit}>
              <span>{
                mode === 'generate'
                  ? t('auth:settings.sshKeys.generateAction')
                  : t('auth:settings.sshKeys.addAction')
              }</span>
            </Button>
          </ModalFooter>
        </> }
    </ModalContent>
  </Modal>
}
