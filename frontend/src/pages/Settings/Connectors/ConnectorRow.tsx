// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { Connector } from '../../../api/routes/connectorRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Card, Chip, Dropdown, Label, Link, Separator, Tooltip } from '@heroui/react'
import { LuListChecks, LuPause, LuPlay, LuRefreshCw, LuSettings, LuTrash2 } from 'react-icons/lu'
import { ConnectorLogo } from './ConnectorLogo'

// Misc
import { formatShortDateTime } from '../../../format/dates'
import { getConnectorUrl } from '../../../urls'
import { accountNameOf, statusColors } from './connectorPresentation'

export type ConnectorAction = 'runChecks' | 'sync' | 'toggle' | 'delete'

type Props = {
  connector: Connector
  // One of the menu's actions is running, which its trigger shows.
  isWorking: boolean
  onAction: (connector: Connector, action: ConnectorAction) => void
}

// One connector in the hub, as Onyx's indexing list shows it: its kind and the account it
// owns, status, last sync, what it recorded, the last error, and a gear menu.
export function ConnectorRow(props: Props) {
  const { t } = useTranslation('connectors')
  const connector = props.connector
  const name = t(`kinds.${connector.kind}.name`)
  const accountName = accountNameOf(connector)
  const toggleLabel = connector.isActive
    ? t('actions.pause')
    : t('actions.resume')
  const ToggleIcon = connector.isActive
    ? LuPause
    : LuPlay
  const menuLabel = t('actions.menu', { name })

  return <Card className='compact p-4'>
    <div className='level items-start gap-4'>
      <div className='flex min-w-0 gap-3'>
        <ConnectorLogo kind={connector.kind} className='mt-0.5 size-6' />
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
          {accountName && <p className='text-sm opacity-70'>{t('row.account', { name: accountName })}</p>}
          {!connector.account && <p className='text-sm opacity-70'>{t('row.waiting')}</p>}
          {connector.lastError && <p className='mt-1 text-sm text-danger break-words'>{connector.lastError}</p>}
        </div>
      </div>
      <div className='flex shrink-0 items-start gap-4'>
        <div className='text-right text-sm'>
          <p className='font-semibold'>{t('row.records', { count: connector.records })}</p>
          <p className='opacity-60'>{
            connector.lastSyncAt
              ? t('row.lastSync', { when: formatShortDateTime(connector.lastSyncAt) })
              : t('row.neverSynced')
          }</p>
        </div>
        <Dropdown>
          <Tooltip delay={300}>
            <Button isIconOnly size='sm' variant='ghost' aria-label={menuLabel} isPending={props.isWorking}>
              <LuSettings className='size-4' aria-hidden />
            </Button>
            <Tooltip.Content>
              <span>{menuLabel}</span>
            </Tooltip.Content>
          </Tooltip>
          <Dropdown.Popover placement='bottom end'>
            <Dropdown.Menu
              aria-label={menuLabel}
              onAction={(key: Key) => props.onAction(connector, String(key) as ConnectorAction)}
            >
              <Dropdown.Item id='runChecks' textValue={t('actions.runChecks')}>
                <LuListChecks className='size-4' aria-hidden />
                <Label>{t('actions.runChecks')}</Label>
              </Dropdown.Item>
              <Dropdown.Item id='sync' textValue={t('actions.sync')} isDisabled={!connector.isActive}>
                <LuRefreshCw className='size-4' aria-hidden />
                <Label>{t('actions.sync')}</Label>
              </Dropdown.Item>
              <Dropdown.Item id='toggle' textValue={toggleLabel}>
                <ToggleIcon className='size-4' aria-hidden />
                <Label>{toggleLabel}</Label>
              </Dropdown.Item>
              <Separator />
              <Dropdown.Item id='delete' textValue={t('actions.delete')} variant='danger'>
                <LuTrash2 className='size-4' aria-hidden />
                <Label>{t('actions.delete')}</Label>
              </Dropdown.Item>
            </Dropdown.Menu>
          </Dropdown.Popover>
        </Dropdown>
      </div>
    </div>
  </Card>
}
