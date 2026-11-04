// Copyright © 2026 Jalapeno Labs

import type { MailAccount } from '../../../api/routes/mailRoutes'

// Core
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// Redux
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import { mailAccountDeleted, mailAccountUpserted, selectAllMailAccounts } from '../../../store/mailAccountsSlice'
import { selectAllMailDomains } from '../../../store/mailDomainsSlice'
import { selectMailServer } from '../../../store/mailServerSlice'

// User interface
import { Alert, Spinner, Tabs } from '@heroui/react'
import { PageTabs } from '../../../components/PageTabs'
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
import { useMailAccountsLoader, useMailDomainsLoader, useMailServerLoader } from '../../../hooks/useServerData'
import { useTabParam } from '../../../hooks/useTabParam'
import { mailAccountKindLabelKeys } from './mailPresentation'
import { useOAuthOutcomeToast } from './useOAuthOutcomeToast'
import { toast } from '../../../toast/toast'

const TABS = [ 'mailboxes', 'domains' ] as const

// `/settings/email`: every mailbox Elysium reads and sends from, and on a tab of its own the
// mail server with the domains it hosts. The open tab lives in the address.
export function ManageEmailPage() {
  const { t } = useTranslation([ 'email', 'common' ])
  const dispatch = useAppDispatch()
  const accounts = useAppSelector(selectAllMailAccounts)
  const server = useAppSelector(selectMailServer)
  const domains = useAppSelector(selectAllMailDomains)
  const status = useMailAccountsLoader()
  // The Mailboxes tab reads both too: creating a mailbox needs a ready server and a domain.
  useMailServerLoader()
  const domainsStatus = useMailDomainsLoader()
  const [ selectedTab, selectTab ] = useTabParam(TABS)
  // Only this page asks, so it stays in SWR rather than Redux.
  const { data: capabilitiesResponse } = useSWR('v1/mail/capabilities', getMailCapabilities)
  const capabilities = capabilitiesResponse?.capabilities

  useOAuthOutcomeToast()

  const confirm = useConfirm()

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

  return <SettingsPage title={t('title')} help={t('help')}>
    <PageTabs
      label={t('tabs.label')}
      selectedKey={selectedTab}
      onSelectionChange={selectTab}
      tabs={[
        {
          id: 'mailboxes',
          label: t('tabs.mailboxes'),
          count: status === 'loaded'
            ? accounts.length
            : null,
        },
        {
          id: 'domains',
          label: t('tabs.domains'),
          count: domainsStatus === 'loaded'
            ? domains.length
            : null,
        },
      ]}
    >
      <Tabs.Panel id='mailboxes'>
        <div className='compact'>
          <MailboxSourceActions
            capabilities={capabilities}
            server={server}
            hasDomains={domains.length > 0}
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
      </Tabs.Panel>
      <Tabs.Panel id='domains'>
        <MailServerSection />
      </Tabs.Panel>
    </PageTabs>
  </SettingsPage>
}
