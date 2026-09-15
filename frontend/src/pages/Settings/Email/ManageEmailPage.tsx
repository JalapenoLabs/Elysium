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
import { Alert, Breadcrumbs, Spinner, toast, useOverlayState } from '@heroui/react'
import { CreateMailboxModal } from './CreateMailboxModal'
import { MailAccountTable } from './MailAccountTable'
import { MailboxSourceActions } from './MailboxSourceActions'
import { MailServerSection } from './MailServerSection'

// Misc
import { getUpstreamErrorMessage } from '../../../api/errors'
import {
  deleteMailAccount,
  getMailCapabilities,
  sendMailTestMessage,
  testMailAccount,
  updateMailAccount,
} from '../../../api/routes/mailRoutes'
import { useConfirm } from '../../../hooks/useConfirm'
import { usePrompt } from '../../../hooks/usePrompt'
import { useMailAccountsLoader } from '../../../hooks/useServerData'
import { UrlTree } from '../../../urls'
import { DISPLAY_NAME_MAX_CHARACTERS } from './mailFormSchemas'
import { mailAccountKindLabelKeys } from './mailPresentation'
import { useOAuthOutcomeToast } from './useOAuthOutcomeToast'

export function ManageEmailPage() {
  const { t } = useTranslation([ 'email', 'settings', 'common' ])
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
  const prompt = usePrompt()
  const createState = useOverlayState()
  // Remounting the form per opening resets it.
  const [ formSession, setFormSession ] = useState(0)

  function openCreate() {
    setFormSession((session) => session + 1)
    createState.open()
  }

  // The one thing about a mailbox worth editing: the name recipients see beside it.
  function promptSenderName(account: MailAccount) {
    prompt({
      title: t('renameForm.title'),
      label: t('renameForm.displayName'),
      description: t('renameForm.hint', { address: account.address }),
      defaultValue: account.displayName,
      isOptional: true,
      maxLength: DISPLAY_NAME_MAX_CHARACTERS,
      onSubmit: async (displayName) => {
        try {
          const response = await updateMailAccount(account.id, { displayName })
          dispatch(mailAccountUpserted(response.account))
          toast.success(t('toasts.updated', { address: response.account.address }))
        }
        catch (error) {
          console.debug('ManageEmailPage failed to save the sender name', { error, accountId: account.id })
          toast.danger(t('common:errors.unexpected'))
          throw error
        }
      },
    })
  }

  // A self-hosted mailbox is destroyed with its mail; an OAuth account is only forgotten.
  function confirmDisconnect(account: MailAccount) {
    const isSelfHosted = account.kind === 'self-hosted'
    confirm({
      title: isSelfHosted
        ? t('disconnect.deleteTitle', { address: account.address })
        : t('disconnect.title', { address: account.address }),
      message: isSelfHosted
        ? t('disconnect.selfHosted')
        : t('disconnect.oauth', { provider: t(mailAccountKindLabelKeys[account.kind]) }),
      tone: 'danger',
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
    const toastId = toast(t('toasts.testing', { address: account.address }), { isLoading: true, timeout: 0 })
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
    const toastId = toast(t('toasts.sending', { address: account.address }), { isLoading: true, timeout: 0 })
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

  async function toggleActive(account: MailAccount) {
    try {
      const response = await updateMailAccount(account.id, { isActive: !account.isActive })
      dispatch(mailAccountUpserted(response.account))
      toast.success(t('toasts.updated', { address: account.address }))
    }
    catch (error) {
      console.debug('ManageEmailPage failed to toggle a mailbox', { error, accountId: account.id })
      toast.danger(t('common:errors.unexpected'))
    }
  }

  return <div className='container'>
    <Breadcrumbs className='compact'>
      <Breadcrumbs.Item href={UrlTree.settings}>{t('settings:title')}</Breadcrumbs.Item>
      <Breadcrumbs.Item>{t('title')}</Breadcrumbs.Item>
    </Breadcrumbs>
    <h1 className='relaxed text-3xl font-bold'>{
      t('title')
    }</h1>

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
        onRename={promptSenderName}
        onToggleActive={toggleActive}
        onDisconnect={confirmDisconnect}
      />}
    </section>

    <CreateMailboxModal
      key={`create-${formSession}`}
      state={createState}
      domains={domains}
    />
  </div>
}
