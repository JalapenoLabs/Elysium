// Copyright © 2026 Jalapeno Labs

import type { MailDomain, MailServerStep } from '../../../api/routes/mailRoutes'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectAllMailAccounts } from '../../../store/mailAccountsSlice'
import { selectAllMailDomains } from '../../../store/mailDomainsSlice'
import { selectMailServer } from '../../../store/mailServerSlice'

// User interface
import { Alert, Button, Card, Spinner, useOverlayState } from '@heroui/react'
import { LuPlus, LuServer } from 'react-icons/lu'
import { AddMailDomainModal } from './AddMailDomainModal'
import { CreateMailServerModal } from './CreateMailServerModal'
import { MailDomainDnsModal } from './MailDomainDnsModal'
import { MailDomainTable } from './MailDomainTable'
import { RemoveMailDomainDialog } from './RemoveMailDomainDialog'

// Misc
import { useMailDomainsLoader, useMailServerLoader } from '../../../hooks/useServerData'

// Creation's steps in order, as the API reports them.
const creationSteps = [
  'preparing',
  'pulling-image',
  'starting',
  'configuring',
  'restarting',
  'adding-domain',
] as const satisfies readonly MailServerStep[]

const stepLabelKeys = {
  'preparing': 'server.steps.preparing',
  'pulling-image': 'server.steps.pulling-image',
  'starting': 'server.steps.starting',
  'configuring': 'server.steps.configuring',
  'restarting': 'server.steps.restarting',
  'adding-domain': 'server.steps.adding-domain',
} as const satisfies Record<MailServerStep, string>

// The mail server and the domains it hosts. Before the server exists this offers to
// create it; while it is created, it follows the steps the event stream reports.
export function MailServerSection() {
  const { t } = useTranslation('email')
  const server = useAppSelector(selectMailServer)
  const domains = useAppSelector(selectAllMailDomains)
  const accounts = useAppSelector(selectAllMailAccounts)
  useMailServerLoader()
  useMailDomainsLoader()

  const createServerState = useOverlayState()
  const addDomainState = useOverlayState()
  const dnsState = useOverlayState()
  const removeState = useOverlayState()
  const [ selectedDomain, setSelectedDomain ] = useState<MailDomain | null>(null)
  // Remounting a form per opening resets it.
  const [ formSession, setFormSession ] = useState(0)

  function openForm(open: () => void) {
    setFormSession((session) => session + 1)
    open()
  }

  const mailboxCounts = useMemo(() => {
    const counts = new Map<string, number>()
    for (const account of accounts) {
      if (account.mailDomainId) {
        counts.set(account.mailDomainId, (counts.get(account.mailDomainId) ?? 0) + 1)
      }
    }
    return counts
  }, [ accounts ])

  let body = <div className='grid place-items-center py-10'>
    <Spinner />
  </div>

  if (server?.state === 'not-created' || server?.state === 'failed') {
    body = <Card className='items-center gap-3 py-10 text-center'>
      <LuServer className='size-8 opacity-60' aria-hidden />
      <div>
        <p className='font-medium'>{t('server.empty.title')}</p>
        <p className='mx-auto mt-1 max-w-lg text-sm opacity-70'>{
          t('server.empty.description')
        }</p>
      </div>
      {server.state === 'failed' && <Alert status='danger' className='max-w-2xl text-left'>
        <Alert.Indicator />
        <Alert.Content>
          <Alert.Title>{t('server.failed.title')}</Alert.Title>
          <Alert.Description className='break-words'>{server.error}</Alert.Description>
        </Alert.Content>
      </Alert>}
      <Button onPress={() => openForm(createServerState.open)}>
        <span>{
          server.state === 'failed'
            ? t('server.failed.action')
            : t('server.empty.action')
        }</span>
      </Button>
    </Card>
  }

  if (server?.state === 'creating' && server.step) {
    const current = creationSteps.indexOf(server.step) + 1
    body = <Card className='flex-row items-center gap-4 p-5'>
      <Spinner size='sm' />
      <div>
        <p className='font-medium'>{t('server.creating')}</p>
        <p className='text-sm opacity-70'>{
          t(stepLabelKeys[server.step])
        } · {
          t('server.stepProgress', { current, total: creationSteps.length })
        }</p>
      </div>
    </Card>
  }

  if (server?.state === 'ready' || server?.state === 'unreachable') {
    body = <>
      {server.state === 'unreachable' && <Alert status='warning' className='compact'>
        <Alert.Indicator />
        <Alert.Content>
          <Alert.Title>{t('server.unreachable.title')}</Alert.Title>
          <Alert.Description>{
            t('server.unreachable.description', { error: server.error ?? '' })
          }</Alert.Description>
        </Alert.Content>
      </Alert>}
      <div className='compact flex flex-wrap items-center justify-between gap-2'>
        <div>
          <h3 className='font-semibold'>{t('domains.heading')}</h3>
          <p className='text-sm opacity-70'>{
            t('server.hostname', { hostname: server.hostname ?? '' })
          }</p>
        </div>
        <Button
          size='sm'
          variant='outline'
          isDisabled={server.state !== 'ready'}
          onPress={() => openForm(addDomainState.open)}
        >
          <LuPlus className='size-4' aria-hidden />
          <span>{t('domains.add')}</span>
        </Button>
      </div>
      <MailDomainTable
        domains={domains}
        mailboxCounts={mailboxCounts}
        onShowDns={(domain) => {
          setSelectedDomain(domain)
          dnsState.open()
        }}
        onRemove={(domain) => {
          setSelectedDomain(domain)
          removeState.open()
        }}
      />
    </>
  }

  return <section className='relaxed'>
    <div className='compact'>
      <h2 className='text-xl font-semibold'>{t('server.heading')}</h2>
      <p className='mt-1 max-w-2xl text-sm opacity-70'>{
        t('server.description')
      }</p>
    </div>
    {body}

    <CreateMailServerModal
      key={`create-server-${formSession}`}
      state={createServerState}
    />
    <AddMailDomainModal
      key={`add-domain-${formSession}`}
      state={addDomainState}
    />
    <MailDomainDnsModal
      state={dnsState}
      domain={selectedDomain}
      hostname={server?.hostname ?? ''}
    />
    <RemoveMailDomainDialog
      state={removeState}
      domain={selectedDomain}
      mailboxCount={selectedDomain
        ? mailboxCounts.get(selectedDomain.id) ?? 0
        : 0}
    />
  </section>
}
