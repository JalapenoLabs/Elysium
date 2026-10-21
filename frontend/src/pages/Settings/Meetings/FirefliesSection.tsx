// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { connectorAccountDeleted, selectAllConnectorAccounts } from '../../../store/connectorAccountsSlice'
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import { selectAllStorageLocations } from '../../../store/storageLocationsSlice'

// User interface
import { Button, Card, Chip, Spinner, toast } from '@heroui/react'
import { FirefliesConnectForm } from './FirefliesConnectForm'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { deleteConnectorAccount } from '../../../api/routes/connectorRoutes'
import { useConnectorAccountsLoader, useStorageLocationsLoader } from '../../../hooks/useServerData'
import { signInChipColors, signInLabelKeys } from '../Browsers/connectorPresentation'

// The person's Fireflies account: the form to connect it, or how its reads are going and the
// button to disconnect it. Its meetings stay on the timeline either way.
export function FirefliesSection() {
  const { t } = useTranslation([ 'meetings', 'browsers', 'common' ])
  const dispatch = useAppDispatch()
  const accountsStatus = useConnectorAccountsLoader()
  const locationsStatus = useStorageLocationsLoader()
  const accounts = useAppSelector(selectAllConnectorAccounts)
  const storageLocations = useAppSelector(selectAllStorageLocations)
  const [ isDisconnecting, setIsDisconnecting ] = useState(false)

  const account = accounts.find((candidate) => candidate.kind === 'fireflies')

  async function disconnect(accountId: string) {
    setIsDisconnecting(true)
    try {
      await deleteConnectorAccount(accountId)
      dispatch(connectorAccountDeleted(accountId))
      toast.success(t('fireflies.disconnectedToast'))
    }
    catch (error) {
      console.debug('FirefliesSection failed to disconnect Fireflies', { error, accountId })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
    finally {
      setIsDisconnecting(false)
    }
  }

  if (accountsStatus === 'loading' || locationsStatus === 'loading') {
    return <div className='relaxed grid place-items-center py-10'>
      <Spinner />
    </div>
  }

  return <Card className='relaxed p-4'>
    <div className='level items-start gap-4'>
      <div className='min-w-0'>
        <div className='flex items-center gap-2'>
          <h2 className='text-base font-semibold'>{t('fireflies.title')}</h2>
          {account && <Chip size='sm' variant='soft' color={signInChipColors[account.signInState]}>{
            t(`browsers:${signInLabelKeys[account.signInState]}`)
          }</Chip>}
        </div>
        <p className='text-sm opacity-70'>{
          account
            ? t('fireflies.connected')
            : t('fireflies.offer')
        }</p>
        {account?.lastError && <p className='mt-1 text-sm text-danger'>{account.lastError}</p>}
      </div>
      {account && <Button
        size='sm'
        variant='outline'
        isPending={isDisconnecting}
        onPress={() => disconnect(account.id)}
      >
        <span>{t('fireflies.disconnect')}</span>
      </Button>}
    </div>
    {!account && <div className='mt-4'>
      <FirefliesConnectForm storageLocations={storageLocations} />
    </div>}
  </Card>
}
