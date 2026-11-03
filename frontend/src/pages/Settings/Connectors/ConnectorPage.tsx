// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router'

// Redux
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import { connectorUpserted, selectCheckRun, selectConnectorById } from '../../../store/connectorsSlice'

// User interface
import { Button, Card, Chip, Link, Spinner, toast } from '@heroui/react'
import { CheckList } from './CheckList'
import { ContentFields } from './ContentFields'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { updateConnector } from '../../../api/routes/connectorRoutes'
import { useConnectorsLoader } from '../../../hooks/useServerData'
import { getGithubCredentialEditUrl, getJiraCredentialEditUrl, getJiraDoneTransitionsUrl } from '../../../urls'
import { statusColors } from './connectorPresentation'
import { useConnectorActions } from './useConnectorActions'
import { useConnectorKinds } from './useConnectorKinds'

// One connector: where it stands, what it reads, and its checks, run live from here.
export function ConnectorPage() {
  const { t } = useTranslation([ 'connectors', 'common' ])
  const { connectorId = '' } = useParams()
  const dispatch = useAppDispatch()
  const status = useConnectorsLoader()
  const definitions = useConnectorKinds()
  const connector = useAppSelector((state) => selectConnectorById(state, connectorId))
  const [ runId, setRunId ] = useState<string | null>(null)
  const run = useAppSelector((state) => selectCheckRun(state, runId))
  const act = useConnectorActions(setRunId)
  const [ content, setContent ] = useState<Record<string, unknown> | null>(null)
  const [ isSaving, setIsSaving ] = useState(false)

  if (status === 'loading' || definitions.isLoading) {
    return <div className='grid place-items-center py-10'><Spinner /></div>
  }
  const definition = connector
    ? definitions.kindOf(connector.kind)
    : null
  if (!connector || !definition) {
    return <p className='container p-6 opacity-70'>{t('common:errors.notFound', { defaultValue: '' })}</p>
  }

  const shownContent = content ?? connector.content
  const accountId = connector.account?.id

  async function saveContent() {
    if (!connector) {
      console.debug('ConnectorPage lost its connector before saving')
      return
    }
    setIsSaving(true)
    try {
      const response = await updateConnector(connector.id, { content: shownContent })
      dispatch(connectorUpserted(response.connector))
      setContent(null)
      toast.success(t('actions.saved'))
    }
    catch (error) {
      console.debug('ConnectorPage failed to save its content', { error, connectorId: connector.id })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
    finally {
      setIsSaving(false)
    }
  }

  // The latest stored results stand until a run started here finishes.
  const outcomes = run
    ? run.checks
    : connector.checks
  const ranAt = run
    ? null
    : connector.checks[0]?.ranAt ?? null

  return <section className='container p-6'>
    <div className='relaxed level items-start'>
      <div>
        <div className='flex items-center gap-2'>
          <h1 className='text-2xl font-semibold'>{t(`kinds.${connector.kind}.name`)}</h1>
          <Chip size='sm' variant='soft' color={statusColors[connector.status]}>{t(`status.${connector.status}`)}</Chip>
        </div>
        <p className='opacity-70'>{connector.account?.name ?? t('row.noAccount')}</p>
      </div>
      <div className='level-right flex-wrap'>
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
      </div>
    </div>

    {connector.lastError && <Card className='relaxed p-4'>
      <p className='text-sm font-medium'>{t('detail.lastError')}</p>
      <p className='text-sm text-danger break-words'>{connector.lastError}</p>
    </Card>}

    <CheckList kind={connector.kind} specs={definition.checks} outcomes={outcomes} ranAt={ranAt} />

    {Boolean(definition.content.length) && <Card className='relaxed p-4'>
      <h2 className='compact font-semibold'>{t('detail.content')}</h2>
      <ContentFields
        kind={connector.kind}
        fields={definition.content}
        account={accountId ? { id: accountId } : null}
        value={connector.kind === 'fireflies'
          ? { storageLocationId: connector.storageLocationId, ...shownContent }
          : shownContent}
        onChange={setContent}
      />
      <div className='mt-4 level-right'>
        <Button isDisabled={!content} isPending={isSaving} onPress={() => void saveContent()}>
          <span>{t('detail.save')}</span>
        </Button>
      </div>
    </Card>}

    {accountId && definition.account === 'atlassian' && <div className='level-left'>
      <Link href={getJiraCredentialEditUrl(accountId)}>{t('detail.editAccount')}</Link>
      {connector.kind === 'jira' && <Link href={getJiraDoneTransitionsUrl(accountId)}>{
        t('detail.doneStatuses')
      }</Link>}
    </div>}
    {accountId && connector.kind === 'github' && <Link href={getGithubCredentialEditUrl(accountId)}>{
      t('detail.editAccount')
    }</Link>}
  </section>
}
