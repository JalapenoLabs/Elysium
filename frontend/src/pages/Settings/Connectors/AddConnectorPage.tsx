// Copyright © 2026 Jalapeno Labs

import type { AccountChoice, ConnectorKind } from '../../../api/routes/connectorRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'
import useSWR from 'swr'

// Redux
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import { selectAllBrowserSessions } from '../../../store/browserSessionsSlice'
import { checkRunLoaded, connectorUpserted, selectCheckRun } from '../../../store/connectorsSlice'

// User interface
import { Button, Card, Link, Radio, RadioGroup, Spinner, toast } from '@heroui/react'
import { AccountFields } from './AccountFields'
import { CheckList } from './CheckList'
import { ContentFields } from './ContentFields'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { checkConnectorDraft, createConnector, listAccounts } from '../../../api/routes/connectorRoutes'
import { useBrowserSessionsLoader } from '../../../hooks/useServerData'
import { UrlTree, getConnectorUrl } from '../../../urls'
import { hasRequiredFailure, offeredAccounts } from './connectorPresentation'
import { useConnectorKinds } from './useConnectorKinds'

// The value the account picker holds for a new account.
const NEW_ACCOUNT = 'new'

// Adding a connector, as Onyx sets one up: pick a kind, then an account the person has or a
// new one, then what it reads, then run its checks live before it is saved.
export function AddConnectorPage() {
  const { t } = useTranslation([ 'connectors', 'common' ])
  const dispatch = useAppDispatch()
  const navigate = useNavigate()
  const definitions = useConnectorKinds()
  useBrowserSessionsLoader()
  const browsers = useAppSelector(selectAllBrowserSessions)
  const accounts = useSWR('v1/accounts', listAccounts)

  const [ kind, setKind ] = useState<ConnectorKind | null>(null)
  const [ accountId, setAccountId ] = useState<string>(NEW_ACCOUNT)
  const [ fields, setFields ] = useState<Record<string, string>>({})
  const [ content, setContent ] = useState<Record<string, unknown>>({})
  const [ runId, setRunId ] = useState<string | null>(null)
  const [ isSaving, setIsSaving ] = useState(false)
  const run = useAppSelector((state) => selectCheckRun(state, runId))

  const definition = kind
    ? definitions.kindOf(kind)
    : null
  const accountDefinition = definition
    ? definitions.accountOf(definition.account)
    : null
  // Each kind says whether it takes a new account, one the person already has, or either.
  const offersNew = definition?.accounts !== 'existing'
  const existing = definition
    ? offeredAccounts(
      definition,
      browsers.map((browser) => ({ id: browser.id, name: browser.name })),
      accounts.data?.accounts ?? [],
    )
    : []
  const account: AccountChoice | null = accountId === NEW_ACCOUNT
    ? (offersNew ? { new: fields } : null)
    : { id: accountId }

  function choose(nextKind: ConnectorKind) {
    setKind(nextKind)
    setAccountId(NEW_ACCOUNT)
    setFields({})
    setContent({})
    setRunId(null)
  }

  async function runChecks() {
    if (!kind || !account) {
      console.debug('AddConnectorPage cannot check without a kind and an account', { kind })
      return
    }
    try {
      const response = await checkConnectorDraft({ kind, account, content })
      dispatch(checkRunLoaded(response.run))
      setRunId(response.run.id)
    }
    catch (error) {
      console.debug('AddConnectorPage failed to start checks', { error })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
  }

  async function save() {
    if (!kind || !account) {
      console.debug('AddConnectorPage cannot save without a kind and an account', { kind })
      return
    }
    setIsSaving(true)
    try {
      const response = await createConnector({ kind, account, content })
      dispatch(connectorUpserted(response.connector))
      toast.success(t('wizard.created'))
      navigate(getConnectorUrl(response.connector.id))
    }
    catch (error) {
      console.debug('AddConnectorPage failed to save', { error })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
    finally {
      setIsSaving(false)
    }
  }

  if (definitions.isLoading) {
    return <div className='grid place-items-center py-10'><Spinner /></div>
  }

  const finished = Boolean(run?.finishedAt)
  const failedRequired = run && definition
    ? hasRequiredFailure(definition.checks, run.checks)
    : false

  return <section className='container p-6'>
    <h1 className='relaxed text-2xl font-semibold'>{t('wizard.title')}</h1>

    <Card className='relaxed p-4'>
      <h2 className='compact font-semibold'>{t('wizard.chooseKind')}</h2>
      <RadioGroup value={kind ?? ''} onChange={(value) => choose(value as ConnectorKind)}>{
        definitions.kinds.map((candidate) => <Radio key={candidate.kind} value={candidate.kind}>
          <Radio.Control>
            <Radio.Indicator />
          </Radio.Control>
          <Radio.Content>
            <span className='font-medium'>{t(`kinds.${candidate.kind}.name`)}</span>
            <span className='block text-sm opacity-70'>{t(`kinds.${candidate.kind}.description`)}</span>
          </Radio.Content>
        </Radio>)
      }</RadioGroup>
    </Card>

    {definition && accountDefinition && <Card className='relaxed p-4'>
      <h2 className='compact font-semibold'>{t('wizard.chooseAccount')}</h2>
      {Boolean(existing.length) && <RadioGroup className='compact' value={accountId} onChange={setAccountId}>
        {existing.map((candidate) => <Radio key={candidate.id} value={candidate.id}>
          <Radio.Control>
            <Radio.Indicator />
          </Radio.Control>
          <Radio.Content>{candidate.name}</Radio.Content>
        </Radio>)}
        {offersNew && <Radio value={NEW_ACCOUNT}>
          <Radio.Control>
            <Radio.Indicator />
          </Radio.Control>
          <Radio.Content>{t('wizard.newAccount')}</Radio.Content>
        </Radio>}
      </RadioGroup>}
      {definition.account === 'browser' && !existing.length && <p className='compact text-sm opacity-70'>{
        t('wizard.noBrowsers')
      }</p>}
      {definition.account === 'browser' && accountId === NEW_ACCOUNT && <p className='text-sm opacity-70'>
        <span>{t('wizard.browserSignIn')}</span>
        {' '}
        <Link href={UrlTree.settingsBrowsers}>{t('wizard.openBrowsers')}</Link>
      </p>}
      {!offersNew && definition.account !== 'browser' && !existing.length && <p className='text-sm opacity-70'>{
        t(`wizard.noAccount.${definition.account}`)
      }</p>}
      {offersNew && accountId === NEW_ACCOUNT && <AccountFields
        definition={accountDefinition}
        values={fields}
        onChange={setFields}
      />}
    </Card>}

    {definition && Boolean(definition.content.length) && <Card className='relaxed p-4'>
      <h2 className='compact font-semibold'>{t('wizard.steps.content')}</h2>
      <ContentFields
        kind={definition.kind}
        fields={definition.content}
        account={account}
        value={content}
        onChange={setContent}
      />
    </Card>}

    {definition && run && <CheckList
      kind={definition.kind}
      specs={definition.checks}
      outcomes={run.checks}
    />}

    {definition && <div className='level-right'>
      <Button variant='outline' isDisabled={!account} onPress={() => void runChecks()}>
        <span>{t('wizard.runChecks')}</span>
      </Button>
      <Button
        isDisabled={!account || !finished}
        isPending={isSaving}
        onPress={() => void save()}
      >
        <span>{failedRequired ? t('wizard.saveAnyway') : t('wizard.save')}</span>
      </Button>
    </div>}
    {failedRequired && <p className='mt-2 text-right text-sm text-warning'>{t('wizard.requiredFailed')}</p>}
  </section>
}
