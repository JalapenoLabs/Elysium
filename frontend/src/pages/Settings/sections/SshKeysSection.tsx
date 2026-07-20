// Copyright © 2026 Jalapeno Labs

import type { SshKey } from '../../../proto/generated/security_pb'

// Core
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

// UI
import { Button } from '@heroui/react'
import { LuPlus, LuTrash2 } from 'react-icons/lu'
import { SettingsSection } from '../../../components/settings/SettingsSection'
import { OhLookAtTheTime } from '../../../components/OhLookAtTheTime'
import { SshKeyModal } from './SshKeyModal'

// Misc
import { useConfirm } from '../../../hooks/useConfirm'
import { useDisplayTimezone } from '../../../hooks/useDisplayTimezone'
import { deleteSshKey, listSshKeys } from '../../../api/routes/userRoutes'

// The SSH keys credential section (#34): the user's keys for git-over-SSH, each
// with its fingerprint and last-used time, plus add (register-existing or
// generate) and remove. Matches the flat, hairline-separated Security Center.
export function SshKeysSection() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const confirm = useConfirm()
  const displayTimezone = useDisplayTimezone()

  const [ keys, setKeys ] = useState<SshKey[]>([])
  const [ isLoaded, setIsLoaded ] = useState(false)
  const [ isModalOpen, setIsModalOpen ] = useState(false)
  const [ errorMessage, setErrorMessage ] = useState<string | null>(null)

  async function loadKeys() {
    try {
      const list = await listSshKeys()
      setKeys(list.keys)
    }
    catch (error) {
      console.debug('ssh keys unavailable', { error })
    }
    finally {
      setIsLoaded(true)
    }
  }

  useEffect(() => {
    loadKeys()
  }, [])

  function onRemove(key: SshKey) {
    confirm({
      title: t('auth:settings.sshKeys.removeTitle'),
      message: t('auth:settings.sshKeys.removeBody', { name: key.name }),
      confirmLabel: t('auth:settings.sshKeys.removeAction'),
      confirmColor: 'danger',
      onConfirm: async () => {
        setErrorMessage(null)
        try {
          await deleteSshKey(key.id)
          await loadKeys()
        }
        catch (error) {
          console.debug('ssh key removal failed', { error })
          setErrorMessage(t('auth:settings.sshKeys.updateFailed'))
        }
      },
    })
  }

  return <>
    <SshKeyModal
      isOpen={isModalOpen}
      onClose={() => setIsModalOpen(false)}
      onCreated={loadKeys}
    />

    <SettingsSection
      title={t('auth:settings.sshKeys.title')}
      description={t('auth:settings.sshKeys.body')}
      action={
        <Button
          size='sm'
          radius='sm'
          variant='bordered'
          startContent={<LuPlus className='h-3.5 w-3.5' />}
          onPress={() => setIsModalOpen(true)}
        >
          <span>{ t('auth:settings.sshKeys.add') }</span>
        </Button>
      }
    >
      { errorMessage
        ? <p className='mb-3 text-sm text-danger'>{ errorMessage }</p>
        : null }
      { keys.length
        ? <div className='divide-y divide-divider'>
          { keys.map((key) =>
            <div key={key.id} className='flex items-center justify-between gap-4 py-3 text-sm'>
              <div className='min-w-0'>
                <p className='truncate'>{ key.name }</p>
                <p className='truncate font-mono text-xs text-default-500'>
                  { key.algorithm }
                  { ' · ' }
                  { key.fingerprint }
                </p>
              </div>
              <div className='flex shrink-0 items-center gap-3'>
                <span className='text-default-500'>
                  { key.lastUsedAt
                    ? <OhLookAtTheTime timestamp={key.lastUsedAt} relative timezone={displayTimezone} />
                    : t('auth:settings.sshKeys.neverUsed') }
                </span>
                <Button
                  isIconOnly
                  size='sm'
                  variant='light'
                  color='danger'
                  aria-label={t('auth:settings.sshKeys.removeAction')}
                  onPress={() => onRemove(key)}
                >
                  <LuTrash2 className='h-4 w-4' />
                </Button>
              </div>
            </div>,
          ) }
        </div>
        : isLoaded
          ? <p className='text-sm text-default-500'>{ t('auth:settings.sshKeys.empty') }</p>
          : null }
    </SettingsSection>
  </>
}
