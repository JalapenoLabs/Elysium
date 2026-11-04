// Copyright © 2026 Jalapeno Labs

import type { ConnectorStatus } from '../../../api/routes/connectorRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectAllConnectors } from '../../../store/connectorsSlice'

// User interface
import { Button, Card, Chip, Label, ListBox, SearchField, Select, Spinner, buttonVariants } from '@heroui/react'
import { Link } from 'react-router'
import { SettingsPage } from '../SettingsPage'
import { ReportIssueButton } from '../../../components/ReportIssueButton'
import { ConnectorRow } from './ConnectorRow'
import { DateText } from '../../../format/DateText'

// Misc
import { listConnectorActivity } from '../../../api/routes/connectorRoutes'
import { useFindHotkey } from '../../../hooks/useFindHotkey'
import { useConnectorsLoader } from '../../../hooks/useServerData'
import { UrlTree } from '../../../urls'
import { filterConnectors, summarize } from './connectorPresentation'
import { useConnectorActions } from './useConnectorActions'

const STATUS_FILTERS = [ 'all', 'active', 'syncing', 'error', 'paused' ] as const satisfies ReadonlyArray<
  ConnectorStatus | 'all'
>

// Settings, Connectors: every connector the person sees, each with the account it owns,
// modeled on Onyx's indexing status. A summary and the recent activity above, then the list
// with search and a status filter.
export function ConnectorsPage() {
  const { t } = useTranslation([ 'connectors', 'common' ])
  const status = useConnectorsLoader()
  const connectors = useAppSelector(selectAllConnectors)
  const activity = useSWR('v1/connectors/activity', listConnectorActivity)
  const { act, isRunning } = useConnectorActions()
  const [ search, setSearch ] = useState('')
  const searchRef = useFindHotkey()
  const [ statusFilter, setStatusFilter ] = useState<ConnectorStatus | 'all'>('all')

  const summary = summarize(connectors)
  const shown = filterConnectors(connectors, search, statusFilter, (kind) => t(`kinds.${kind}.name`))
  const nameOf = new Map(connectors.map((connector) => [ connector.id, t(`kinds.${connector.kind}.name`) ]))

  return <SettingsPage
    title={t('page.title')}
    primaryAction={<Link to={UrlTree.settingsConnectorNew} className={buttonVariants({ variant: 'primary' })}>
      <span>{t('page.add')}</span>
    </Link>}
  >
    <div className='relaxed grid gap-4 md:grid-cols-[2fr_3fr]'>
      <Card className='p-4'>
        <p className='text-lg font-semibold'>{
          t('summary.active', { active: summary.active, total: summary.total })
        }</p>
        <p className='opacity-70'>{t('summary.records', { count: summary.records })}</p>
        {Boolean(summary.errors) && <p className='text-danger'>{t('summary.errors', { count: summary.errors })}</p>}
      </Card>
      <Card className='p-4'>
        <h2 className='compact font-semibold'>{t('activity.title')}</h2>
        {!activity.data?.runs.length && <p className='text-sm opacity-70'>{t('activity.empty')}</p>}
        <ul className='flex max-h-48 flex-col gap-2 overflow-y-auto'>{
          activity.data?.runs.slice(0, 10).map((run) => {
            const connector = connectors.find((candidate) => candidate.id === run.connectorId)
            return <li key={run.id} className='level gap-2 text-sm'>
              <span className='min-w-0'>
                <span className='font-medium'>{nameOf.get(run.connectorId) ?? ''}</span>
                <span className='ml-2 opacity-70'>{
                  run.outcome === 'failed'
                    ? run.error ?? t('activity.failed')
                    : t('activity.succeeded', { count: run.records })
                }</span>
              </span>
              <span className='flex shrink-0 items-center gap-2'>
                <DateText value={run.startedAt} className='opacity-60' />
                {run.outcome === 'failed' && connector && <>
                  <ReportIssueButton
                    isIconOnly
                    subject={{ kind: 'connectorSync', connectorId: connector.id, runId: run.id }}
                  />
                  <Button
                    size='sm'
                    variant='ghost'
                    isPending={isRunning(connector, 'sync')}
                    onPress={() => act(connector, 'sync')}
                  >
                    <span>{t('activity.retry')}</span>
                  </Button>
                </>}
              </span>
            </li>
          })
        }</ul>
      </Card>
    </div>

    <div className='relaxed flex flex-wrap items-end gap-3'>
      <SearchField
        id='connectors-search'
        autoComplete='off'
        className='min-w-64 flex-1'
        aria-label={t('page.search')}
        value={search}
        onChange={setSearch}
      >
        <SearchField.Group>
          <SearchField.SearchIcon />
          <SearchField.Input ref={searchRef} placeholder={t('page.search')} />
          <SearchField.ClearButton />
        </SearchField.Group>
      </SearchField>
      <Select
        id='connectors-status'
        className='w-48'
        value={statusFilter}
        onChange={(key) => setStatusFilter(key as ConnectorStatus | 'all')}
      >
        <Label className='sr-only'>{t('page.allStatuses')}</Label>
        <Select.Trigger>
          <Select.Value />
          <Select.Indicator />
        </Select.Trigger>
        <Select.Popover>
          <ListBox>{
            STATUS_FILTERS.map((filter) => {
              const label = filter === 'all'
                ? t('page.allStatuses')
                : t(`status.${filter}`)
              return <ListBox.Item key={filter} id={filter} textValue={label}>
                {label}
                <ListBox.ItemIndicator />
              </ListBox.Item>
            })
          }</ListBox>
        </Select.Popover>
      </Select>
      {Boolean(summary.errors) && <Chip variant='soft' color='danger'>{
        t('summary.errors', { count: summary.errors })
      }</Chip>}
    </div>

    {status === 'loading' && <div className='grid place-items-center py-10'><Spinner /></div>}
    {status === 'loaded' && !connectors.length && <p className='opacity-70'>{t('page.empty')}</p>}
    {status === 'loaded' && Boolean(connectors.length) && !shown.length && <p className='opacity-70'>{
      t('page.noMatches')
    }</p>}
    {shown.map((connector) => <ConnectorRow
      key={connector.id}
      connector={connector}
      isWorking={isRunning(connector)}
      onAction={act}
    />)}
  </SettingsPage>
}
