// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate, useParams } from 'react-router'

// Redux
import { selectCheckRun, selectConnectorById } from '../../../store/connectorsSlice'
import { useAppSelector } from '../../../store/hooks'

// User interface
import { Button, Card, Chip, Link, Spinner } from '@heroui/react'
import { ReportIssueButton } from '../../../components/ReportIssueButton'
import { SettingsPage } from '../SettingsPage'
import { BrowserSignIn } from './BrowserSignIn'
import { CheckList } from './CheckList'
import { ConnectorContentForm } from './ConnectorContentForm'
import { ConnectorLogo } from './ConnectorLogo'

// Misc
import { useConnectorsLoader } from '../../../hooks/useServerData'
import {
  UrlTree,
  getGithubCredentialEditUrl,
  getJiraCredentialEditUrl,
  getJiraDoneTransitionsUrl,
} from '../../../urls'
import { accountNameOf, statusColors } from './connectorPresentation'
import { useConnectorActions } from './useConnectorActions'
import { useConnectorKinds } from './useConnectorKinds'

// One connector: where it stands; for one read through a browser, its browser to sign in on;
// what it reads; and its checks, run live from here. A new browser connector is set up here.
export function ConnectorPage() {
  const { t } = useTranslation([ 'connectors', 'common' ])
  const { connectorId = '' } = useParams()
  const status = useConnectorsLoader()
  const definitions = useConnectorKinds()
  const connector = useAppSelector((state) => selectConnectorById(state, connectorId))
  const [ runId, setRunId ] = useState<string | null>(null)
  const run = useAppSelector((state) => selectCheckRun(state, runId))
  const navigate = useNavigate()
  const act = useConnectorActions({
    onRunStarted: setRunId,
    onDeleted: () => void navigate(UrlTree.settingsConnectors),
  })
  const parents = [{ label: t('page.title'), href: UrlTree.settingsConnectors }]

  if (status === 'loading' || definitions.isLoading) {
    return <div className='grid place-items-center py-10'><Spinner /></div>
  }
  const definition = connector
    ? definitions.kindOf(connector.kind)
    : null
  if (!connector || !definition) {
    return <SettingsPage title={t('detail.missingTitle')} parents={parents}>
      <p className='opacity-70'>{t('detail.missing')}</p>
    </SettingsPage>
  }

  // The latest stored results stand until a run started here finishes.
  const outcomes = run
    ? run.checks
    : connector.checks
  const ranAt = run
    ? null
    : connector.checks[0]?.ranAt ?? null
  const accountId = connector.account?.id

  return <SettingsPage
    title={t(`kinds.${connector.kind}.name`)}
    icon={<ConnectorLogo kind={connector.kind} className='size-8' />}
    parents={parents}
    actions={<>
      <Button variant='outline' onPress={() => act(connector, 'runChecks')}>
        <span>{t('actions.runChecks')}</span>
      </Button>
      <Button variant='outline' isDisabled={!connector.isActive} onPress={() => act(connector, 'sync')}>
        <span>{t('actions.sync')}</span>
      </Button>
      <Button variant='outline' onPress={() => act(connector, 'toggle')}>
        <span>{connector.isActive ? t('actions.pause') : t('actions.resume')}</span>
      </Button>
      <Button variant='danger' onPress={() => act(connector, 'delete')}>
        <span>{t('actions.delete')}</span>
      </Button>
    </>}
  >
    <div className='relaxed flex flex-wrap items-center gap-2'>
      <Chip size='sm' variant='soft' color={statusColors[connector.status]}>{t(`status.${connector.status}`)}</Chip>
      {accountNameOf(connector) && <span className='text-sm opacity-70'>{accountNameOf(connector)}</span>}
      {!connector.account && <span className='text-sm opacity-70'>{t('row.waiting')}</span>}
    </div>

    {connector.lastError && <Card className='relaxed p-4'>
      <div className='level gap-2'>
        <p className='text-sm font-medium'>{t('detail.lastError')}</p>
        <ReportIssueButton subject={{ kind: 'connectorSync', connectorId: connector.id }} />
      </div>
      <p className='text-sm text-danger break-words'>{connector.lastError}</p>
    </Card>}

    {definition.account === 'browser' && <BrowserSignIn connector={connector} />}

    {Boolean(definition.content.length) && <ConnectorContentForm connector={connector} definition={definition} />}

    <CheckList
      kind={connector.kind}
      specs={definition.checks}
      outcomes={outcomes}
      ranAt={ranAt}
      reportSubjectOf={(checkId) => run
        ? { kind: 'checkRun', runId: run.id, checkId }
        : { kind: 'connectorCheck', connectorId: connector.id, checkId }}
    />

    {accountId && connector.kind === 'jira' && <div className='level-left'>
      <Link href={getJiraCredentialEditUrl(accountId)}>{t('detail.editAccount')}</Link>
      <Link href={getJiraDoneTransitionsUrl(accountId)}>{t('detail.doneStatuses')}</Link>
    </div>}
    {accountId && connector.kind === 'github' && <Link href={getGithubCredentialEditUrl(accountId)}>{
      t('detail.editAccount')
    }</Link>}
  </SettingsPage>
}
