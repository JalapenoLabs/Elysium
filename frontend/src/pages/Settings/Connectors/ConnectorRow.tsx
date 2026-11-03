// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { Connector } from '../../../api/routes/connectorRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Card, Chip, Dropdown, Label, Link } from '@heroui/react'
import { LuSettings } from 'react-icons/lu'

// Misc
import { getConnectorUrl } from '../../../urls'
import { statusColors } from './connectorPresentation'

export type ConnectorAction = 'runChecks' | 'sync' | 'toggle' | 'delete'

type Props = {
  connector: Connector
  onAction: (connector: Connector, action: ConnectorAction) => void
}

// One connector in the hub, as Onyx's indexing list shows it: its kind and account, status,
// last sync, what it recorded, the last error, and a gear menu.
export function ConnectorRow(props: Props) {
  const { t } = useTranslation('connectors')
  const connector = props.connector
  const name = t(`kinds.${connector.kind}.name`)
  const toggleLabel = connector.isActive
    ? t('actions.pause')
    : t('actions.resume')

  return <Card className='compact p-4'>
    <div className='level items-start gap-4'>
      <div className='min-w-0'>
        <div className='flex flex-wrap items-center gap-2'>
          <Link href={getConnectorUrl(connector.id)} className='font-semibold'>{name}</Link>
          <Chip size='sm' variant='soft' color={statusColors[connector.status]}>{
            t(`status.${connector.status}`)
          }</Chip>
          <Chip size='sm' variant='soft'>{
            connector.scope === 'workspace'
              ? t('row.workspace')
              : t('row.personal')
          }</Chip>
        </div>
        <p className='text-sm opacity-70'>{
          connector.account
            ? t('row.account', { name: connector.account.name })
            : t('row.noAccount')
        }</p>
        {connector.lastError && <p className='mt-1 text-sm text-danger break-words'>{connector.lastError}</p>}
      </div>
      <div className='flex shrink-0 items-start gap-4'>
        <div className='text-right text-sm'>
          <p className='font-semibold'>{t('row.records', { count: connector.records })}</p>
          <p className='opacity-60'>{
            connector.lastSyncAt
              ? t('row.lastSync', { when: new Date(connector.lastSyncAt).toLocaleString() })
              : t('row.neverSynced')
          }</p>
        </div>
        <Dropdown>
          <Button isIconOnly size='sm' variant='ghost' aria-label={t('actions.menu', { name })}>
            <LuSettings className='size-4' aria-hidden />
          </Button>
          <Dropdown.Popover placement='bottom end'>
            <Dropdown.Menu onAction={(key: Key) => props.onAction(connector, String(key) as ConnectorAction)}>
              <Dropdown.Item id='runChecks' textValue={t('actions.runChecks')}>
                <Label>{t('actions.runChecks')}</Label>
              </Dropdown.Item>
              <Dropdown.Item id='sync' textValue={t('actions.sync')} isDisabled={!connector.isActive}>
                <Label>{t('actions.sync')}</Label>
              </Dropdown.Item>
              <Dropdown.Item id='toggle' textValue={toggleLabel}>
                <Label>{toggleLabel}</Label>
              </Dropdown.Item>
              <Dropdown.Item id='delete' textValue={t('actions.delete')} variant='danger'>
                <Label>{t('actions.delete')}</Label>
              </Dropdown.Item>
            </Dropdown.Menu>
          </Dropdown.Popover>
        </Dropdown>
      </div>
    </div>
  </Card>
}
