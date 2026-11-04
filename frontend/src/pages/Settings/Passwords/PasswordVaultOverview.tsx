// Copyright © 2026 Jalapeno Labs

import type { PasswordVault, VaultStatus } from '../../../api/routes/passwordVaultRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useConfirm } from '../../../hooks/useConfirm'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { passwordVaultDeleted, passwordVaultStatusUpdated } from '../../../store/passwordVaultSlice'

// User interface
import { Alert, Button, Chip } from '@heroui/react'
import { LuRefreshCw } from 'react-icons/lu'

// Misc
import { formatDateTime } from '../../../format/dates'
import { getApiErrorMessage } from '../../../api/errors'
import { deletePasswordVault, syncPasswordVault } from '../../../api/routes/passwordVaultRoutes'
import { toast } from '../../../toast/toast'
import { serverLabelKeys, stateChipColors, stateLabelKeys } from './passwordsPresentation'

type Props = {
  vault: PasswordVault
  status: VaultStatus
  onEdit: () => void
}

// The connected account, what the CLI is doing, and what an admin can do about it: sync now,
// change the credentials, or remove the vault once the confirmation dialog agrees.
export function PasswordVaultOverview(props: Props) {
  const { t } = useTranslation([ 'passwords', 'common' ])
  const dispatch = useAppDispatch()
  const confirm = useConfirm()
  const [ isSyncing, setIsSyncing ] = useState(false)
  const { vault, status } = props

  async function sync() {
    setIsSyncing(true)
    try {
      const response = await syncPasswordVault()
      dispatch(passwordVaultStatusUpdated(response.status))
      toast.success(t('toasts.synced'))
    }
    catch (error) {
      console.debug('PasswordVaultOverview could not sync the vault', { error })
      toast.danger(t('toasts.failed'), { description: getApiErrorMessage(error) ?? undefined })
    }
    setIsSyncing(false)
  }

  function remove() {
    confirm({
      title: t('remove.title', { email: vault.accountEmail }),
      message: t('remove.body', { email: vault.accountEmail }),
      isIrreversible: true,
      confirmText: t('actions.remove'),
      onConfirm: async () => {
        try {
          await deletePasswordVault()
        }
        catch (error) {
          console.debug('PasswordVaultOverview could not remove the vault', { error })
          toast.danger(t('toasts.failed'), { description: getApiErrorMessage(error) ?? undefined })
          // Keeps the dialog open to try again.
          throw error
        }
        dispatch(passwordVaultDeleted())
        toast.success(t('toasts.removed'))
      },
    })
  }

  const syncedText = status.lastSyncedAt
    ? t('overview.lastSynced', { syncedAt: formatDateTime(status.lastSyncedAt) })
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

    <div className='flex flex-wrap items-center justify-between gap-3'>
      <p className='text-sm opacity-70'>{syncedText}</p>
      <div className='flex flex-wrap gap-2'>
        <Button variant='tertiary' onPress={remove}>
          <span>{t('actions.remove')}</span>
        </Button>
        <Button variant='outline' isDisabled={isSyncing} onPress={props.onEdit}>
          <span>{t('actions.edit')}</span>
        </Button>
        <Button
          variant='outline'
          isPending={isSyncing}
          isDisabled={status.state !== 'ready'}
          onPress={sync}
        >
          <LuRefreshCw />
          <span>{t('actions.syncNow')}</span>
        </Button>
      </div>
    </div>
  </section>
}
