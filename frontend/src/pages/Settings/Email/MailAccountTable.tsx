// Copyright © 2026 Jalapeno Labs

import type { MailAccount } from '../../../api/routes/mailRoutes'
import type { MailAccountActions } from './MailAccountRowActions'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Chip, Tooltip } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { MailAccountRowActions } from './MailAccountRowActions'

// Misc
import { useSmartTableLabels } from '../../../hooks/useSmartTableLabels'
import {
  getMailAccountHealth,
  mailAccountHealthChipColors,
  mailAccountHealthLabelKeys,
  mailAccountKindIcons,
  mailAccountKindLabelKeys,
} from './mailPresentation'

type Props = MailAccountActions & {
  accounts: MailAccount[]
}

const MAIL_COLUMN_KEYS = [ 'address', 'provider', 'status', 'lastChecked', 'rowActions' ] as const
type MailColumnKey = typeof MAIL_COLUMN_KEYS[number]

const columnLabelKeys = {
  address: 'table.address',
  provider: 'table.provider',
  status: 'table.status',
  lastChecked: 'table.lastChecked',
  rowActions: 'common:actions.moreActions',
} as const satisfies Record<MailColumnKey, string>

// Starting widths in pixels, summing to less than the settings content column.
const columnSizes = {
  address: 320,
  provider: 170,
  status: 150,
  lastChecked: 220,
  rowActions: 64,
} as const satisfies Record<MailColumnKey, number>

export function MailAccountTable(props: Props) {
  const { t, i18n } = useTranslation([ 'email', 'common' ])
  const labels = useSmartTableLabels()
  const [ search, setSearch ] = useState('')

  const managedColumns = useMemo(() => {
    // Timestamps arrive as UTC; this is the one place they become local time.
    const dateFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium', timeStyle: 'short' })
    const lastCheckedText = (account: MailAccount) => account.lastCheckedAt
      ? dateFormatter.format(new Date(account.lastCheckedAt))
      : t('table.never')

    const renderCell = {
      address: (account: MailAccount) => <div>
        <div className='font-medium'>{account.address}</div>
        {account.displayName && <div className='max-w-xs truncate text-xs opacity-70'>{
          account.displayName
        }</div>}
      </div>,
      provider: (account: MailAccount) => {
        const Icon = mailAccountKindIcons[account.kind]
        return <span className='inline-flex items-center gap-2'>
          <Icon className='size-4 opacity-80' aria-hidden />
          <span>{t(mailAccountKindLabelKeys[account.kind])}</span>
        </span>
      },
      status: (account: MailAccount) => {
        const health = getMailAccountHealth(account)
        const chip = <Chip size='sm' variant='soft' color={mailAccountHealthChipColors[health]}>{
          t(mailAccountHealthLabelKeys[health])
        }</Chip>

        // A failing mailbox explains itself on hover.
        if (health !== 'failing' || !account.lastError) {
          return chip
        }
        return <Tooltip delay={200}>
          <Tooltip.Trigger>{chip}</Tooltip.Trigger>
          <Tooltip.Content className='max-w-sm'>
            <span>{account.lastError}</span>
          </Tooltip.Content>
        </Tooltip>
      },
      lastChecked: lastCheckedText,
      rowActions: (account: MailAccount) => <MailAccountRowActions
        account={account}
        onTest={props.onTest}
        onSendTest={props.onSendTest}
        onRename={props.onRename}
        onToggleActive={props.onToggleActive}
        onDisconnect={props.onDisconnect}
      />,
    } satisfies Record<MailColumnKey, (account: MailAccount) => unknown>

    // Search matches what the user sees.
    const searchValue = {
      address: (account: MailAccount) => `${account.address} ${account.displayName}`,
      provider: (account: MailAccount) => t(mailAccountKindLabelKeys[account.kind]),
      status: (account: MailAccount) => t(mailAccountHealthLabelKeys[getMailAccountHealth(account)]),
      lastChecked: lastCheckedText,
      rowActions: null,
    } satisfies Record<MailColumnKey, ((account: MailAccount) => string) | null>

    return createManagedColumns<MailAccount, MailColumnKey>({
      columnKeys: MAIL_COLUMN_KEYS,
      getColumnLabel: (columnKey) => t(columnLabelKeys[columnKey]),
      getSearchKey: (columnKey) => searchValue[columnKey]
        ? columnKey
        : null,
      getSearchValue: (columnKey) => searchValue[columnKey],
      createColumnDef: ({ columnKey, columnId, columnLabel }) => {
        const toSearchText = searchValue[columnKey]
        if (!toSearchText) {
          return {
            id: columnId,
            header: () => <span className='sr-only'>{columnLabel}</span>,
            enableSorting: false,
            size: columnSizes[columnKey],
            cell: ({ row }) => renderCell[columnKey](row.original),
          }
        }

        return {
          id: columnId,
          header: columnLabel,
          accessorFn: toSearchText,
          size: columnSizes[columnKey],
          cell: ({ row }) => renderCell[columnKey](row.original),
        }
      },
    })
  }, [ t, i18n.language, props.onTest, props.onSendTest, props.onRename, props.onToggleActive, props.onDisconnect ])

  if (!props.accounts.length) {
    return <p className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'>{
      t('table.empty')
    }</p>
  }

  return <SmartTable
    ids={{
      tableElementId: 'mail-accounts-table',
      tableLocalStorageId: 'elysium.settings.email.table',
    }}
    tableAriaLabel={t('table.label')}
    data={props.accounts}
    managedColumns={managedColumns}
    getRowId={(account) => account.id}
    labels={labels}
    search={{
      value: search,
      onChange: setSearch,
    }}
    enableSorting
    stickyHeader={false}
  />
}
