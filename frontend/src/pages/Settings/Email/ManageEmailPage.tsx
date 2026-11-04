// Copyright © 2026 Jalapeno Labs

import type { MailAccount } from '../../../api/routes/mailRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// Redux
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import { mailAccountDeleted, mailAccountUpserted, selectAllMailAccounts } from '../../../store/mailAccountsSlice'
import { selectAllMailDomains } from '../../../store/mailDomainsSlice'
import { selectMailServer } from '../../../store/mailServerSlice'

// User interface
import { Alert, Spinner, useOverlayState } from '@heroui/react'
import { CreateMailboxModal } from './CreateMailboxModal'
import { MailAccountTable } from './MailAccountTable'
import { MailboxSourceActions } from './MailboxSourceActions'
import { MailServerSection } from './MailServerSection'
import { SettingsPage } from '../SettingsPage'

// Misc
import { getUpstreamErrorMessage } from '../../../api/errors'
import {
  deleteMailAccount,
  getMailCapabilities,
  sendMailTestMessage,
  testMailAccount,
} from '../../../api/routes/mailRoutes'
import { useConfirm } from '../../../hooks/useConfirm'
import { useMailAccountsLoader } from '../../../hooks/useServerData'
import { mailAccountKindLabelKeys } from './mailPresentation'
import { useOAuthOutcomeToast } from './useOAuthOutcomeToast'
import { toast } from '../../../toast/toast'

export function ManageEmailPage() {
  const { t } = useTranslation([ 'email', 'common' ])
  const dispatch = useAppDispatch()
  const accounts = useAppSelector(selectAllMailAccounts)
  const server = useAppSelector(selectMailServer)
  const domains = useAppSelector(selectAllMailDomains)
  const status = useMailAccountsLoader()
  // Only this page asks, so it stays in SWR rather than Redux.
  const { data: capabilitiesResponse } = useSWR('v1/mail/capabilities', getMailCapabilities)
  const capabilities = capabilitiesResponse?.capabilities

  useOAuthOutcomeToast()

  const confirm = useConfirm()
  const createState = useOverlayState()
  // Remounting the form per opening resets it.
  const [ formSession, setFormSession ] = useState(0)

  function openCreate() {
    setFormSession((session) => session + 1)
    createState.open()
  }

  // A self-hosted mailbox is destroyed with its mail; an OAuth account is only forgotten.
  function confirmDisconnect(account: MailAccount) {
    const isSelfHosted = account.kind === 'self-hosted'
    confirm({
      title: isSelfHosted
        ? t('disconnect.deleteTitle', { address: account.address })
        : t('disconnect.title', { address: account.address }),
      message: isSelfHosted
        ? t('disconnect.selfHosted', { address: account.address })
        : t('disconnect.oauth', {
          address: account.address,
          provider: t(mailAccountKindLabelKeys[account.kind]),
        }),
      tone: 'danger',
      // Deleting a self-hosted mailbox deletes its mail; an OAuth account can be connected again.
      isIrreversible: isSelfHosted,
      confirmText: isSelfHosted
        ? t('actions.deleteMailbox')
        : t('actions.disconnect'),
      onConfirm: async () => {
        try {
          await deleteMailAccount(account.id)
        }
        catch (error) {
          const message = getUpstreamErrorMessage(error)
          if (!message) {
            console.debug('ManageEmailPage failed to disconnect the account', { error, accountId: account.id })
          }
          toast.danger(message ?? t('common:errors.unexpected'))
          throw error
        }
        dispatch(mailAccountDeleted(account.id))
        toast.success(t('toasts.disconnected', { address: account.address }))
      },
    })
  }

  async function runConnectionTest(account: MailAccount) {
    const toastId = toast.loading(t('toasts.testing', { address: account.address }))
    try {
      const response = await testMailAccount(account.id)
      dispatch(mailAccountUpserted(response.account))
      toast.close(toastId)

      if (response.account.lastError) {
        toast.danger(t('toasts.testFailed', { address: account.address }), {
          description: response.account.lastError,
        })
        return
      }
      toast.success(t('toasts.testPassed', { address: account.address }))
    }
    catch (error) {
      toast.close(toastId)
      console.debug('ManageEmailPage failed to test a mailbox', { error, accountId: account.id })
      toast.danger(t('common:errors.unexpected'))
    }
  }

  async function sendTestMessage(account: MailAccount) {
    const toastId = toast.loading(t('toasts.sending', { address: account.address }))
    try {
      const response = await sendMailTestMessage(account.id)
      toast.close(toastId)
      toast.success(t('toasts.sent', { address: response.sentTo }))
    }
    catch (error) {
      toast.close(toastId)
      const message = getUpstreamErrorMessage(error)
      if (!message) {
        console.debug('ManageEmailPage failed to send a test message', { error, accountId: account.id })
      }
      toast.danger(t('toasts.sendFailed'), {
        description: message ?? t('common:errors.unexpected'),
      })
    }
  }

  return <SettingsPage title={t('title')}>
    <MailServerSection />

    <section>
      <div className='compact flex flex-wrap items-start justify-between gap-4'>
        <div>
          <h2 className='text-xl font-semibold'>{
            t('mailboxes.heading')
          }</h2>
          <p className='mt-1 max-w-2xl text-sm opacity-70'>{
            t('mailboxes.description')
          }</p>
        </div>
        <MailboxSourceActions
          capabilities={capabilities}
          server={server}
          hasDomains={domains.length > 0}
          onCreateMailbox={openCreate}
        />
      </div>

      {capabilities?.brokerError && <Alert status='warning' className='compact'>
        <Alert.Indicator />
        <Alert.Content>
          <Alert.Title>{
            t('availability.brokerUnreachable.title')
          }</Alert.Title>
          <Alert.Description>{
            t('availability.brokerUnreachable.description', { error: capabilities.brokerError })
          }</Alert.Description>
        </Alert.Content>
      </Alert>}

      {status === 'loading' && <div className='grid place-items-center py-16'>
        <Spinner />
      </div>}

      {status === 'failed' && <p className='py-10 text-center text-sm text-danger'>{
        t('table.loadError')
      }</p>}

      {status === 'loaded' && <MailAccountTable
        accounts={accounts}
        onTest={runConnectionTest}
        onSendTest={sendTestMessage}
        onDisconnect={confirmDisconnect}
      />}
    </section>

    <CreateMailboxModal
      key={`create-${formSession}`}
      state={createState}
      domains={domains}
    />
  </SettingsPage>
}
