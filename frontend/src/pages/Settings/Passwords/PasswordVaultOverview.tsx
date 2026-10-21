// Copyright © 2026 Jalapeno Labs

import type { PasswordVault, VaultStatus } from '../../../api/routes/passwordVaultRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { passwordVaultDeleted, passwordVaultStatusUpdated } from '../../../store/passwordVaultSlice'

// User interface
import { Alert, Button, Chip, toast } from '@heroui/react'
import { LuRefreshCw } from 'react-icons/lu'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { deletePasswordVault, syncPasswordVault } from '../../../api/routes/passwordVaultRoutes'
import { serverLabelKeys, stateChipColors, stateLabelKeys } from './passwordsPresentation'

type Props = {
  vault: PasswordVault
  status: VaultStatus
  onEdit: () => void
}

const dateTimeFormatter = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' })

// The connected account, what the CLI is doing, and what an admin can do about it: sync now,
// change the credentials, or remove the vault after a second press.
export function PasswordVaultOverview(props: Props) {
  const { t } = useTranslation([ 'passwords', 'common' ])
  const dispatch = useAppDispatch()
  const [ busyAction, setBusyAction ] = useState<'sync' | 'remove' | null>(null)
  const [ isConfirmingRemove, setIsConfirmingRemove ] = useState(false)
  const { vault, status } = props

  async function sync() {
    setBusyAction('sync')
    try {
      const response = await syncPasswordVault()
      dispatch(passwordVaultStatusUpdated(response.status))
      toast.success(t('toasts.synced'))
    }
    catch (error) {
      console.debug('PasswordVaultOverview could not sync the vault', { error })
      toast.danger(t('toasts.failed'), { description: getApiErrorMessage(error) ?? undefined })
    }
    setBusyAction(null)
  }

  async function remove() {
    setBusyAction('remove')
    try {
      await deletePasswordVault()
      dispatch(passwordVaultDeleted())
      toast.success(t('toasts.removed'))
    }
    catch (error) {
      console.debug('PasswordVaultOverview could not remove the vault', { error })
      toast.danger(t('toasts.failed'), { description: getApiErrorMessage(error) ?? undefined })
      setBusyAction(null)
    }
  }

  const syncedText = status.lastSyncedAt
    ? t('overview.lastSynced', { syncedAt: dateTimeFormatter.format(new Date(status.lastSyncedAt)) })
    : t('overview.neverSynced')

  return <section className='relaxed rounded-xl border border-separator p-6'>
    <div className='compact flex flex-wrap items-start justify-between gap-4'>
      <div>
        <p className='text-sm opacity-70'>{t('overview.account')}</p>
        <p className='text-2xl font-semibold'>{vault.accountEmail}</p>
        <p className='text-sm opacity-70'>{
          vault.serverUrl ?? t(serverLabelKeys[vault.server])
        }</p>
      </div>
      <Chip size='sm' variant='soft' color={stateChipColors[status.state]}>{
        t(stateLabelKeys[status.state])
      }</Chip>
    </div>

    {status.state === 'failed' && <Alert className='compact' status='danger'>
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Title>{t('overview.failedTitle')}</Alert.Title>
        <Alert.Description>{status.error}</Alert.Description>
      </Alert.Content>
    </Alert>}
    {status.state === 'starting' && <p className='compact text-sm opacity-70'>{
      t('overview.startingDescription')
    }</p>}

    {isConfirmingRemove && <Alert className='compact' status='warning'>
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Description>{t('remove.confirm')}</Alert.Description>
      </Alert.Content>
    </Alert>}

    <div className='flex flex-wrap items-center justify-between gap-3'>
      <p className='text-sm opacity-70'>{syncedText}</p>
      <div className='flex flex-wrap gap-2'>
        {isConfirmingRemove
          ? <>
            <Button variant='tertiary' onPress={() => setIsConfirmingRemove(false)}>
              <span>{t('common:actions.cancel')}</span>
            </Button>
            <Button
              variant='danger'
              isPending={busyAction === 'remove'}
              onPress={remove}
            >
              <span>{t('actions.confirmRemove')}</span>
            </Button>
          </>
          : <>
            <Button variant='tertiary' onPress={() => setIsConfirmingRemove(true)}>
              <span>{t('actions.remove')}</span>
            </Button>
            <Button variant='outline' isDisabled={busyAction !== null} onPress={props.onEdit}>
              <span>{t('actions.edit')}</span>
            </Button>
            <Button
              variant='outline'
              isPending={busyAction === 'sync'}
              isDisabled={busyAction !== null || status.state !== 'ready'}
              onPress={sync}
            >
              <LuRefreshCw />
              <span>{t('actions.syncNow')}</span>
            </Button>
          </>}
      </div>
    </div>
  </section>
}
