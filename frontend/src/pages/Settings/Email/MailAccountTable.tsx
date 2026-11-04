// Copyright © 2026 Jalapeno Labs

import type { DataTableAction } from '@jalapenolabs/uikit'
import type { MailAccount } from '../../../api/routes/mailRoutes'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Chip, Link, Tooltip } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { LuPlugZap, LuSend, LuTrash2, LuUnplug } from 'react-icons/lu'

// Misc
import { DateText } from '../../../format/DateText'
import { formatDateTime } from '../../../format/dates'
import { useSmartTableLabels } from '../../../hooks/useSmartTableLabels'
import { getMailboxEditUrl } from '../../../urls'
import {
  getMailAccountHealth,
  mailAccountHealthChipColors,
  mailAccountHealthLabelKeys,
  mailAccountKindIcons,
  mailAccountKindLabelKeys,
} from './mailPresentation'

type Props = {
  accounts: MailAccount[]
  onTest: (account: MailAccount) => Promise<void>
  onSendTest: (account: MailAccount) => Promise<void>
  onDisconnect: (account: MailAccount) => void
}

const MAIL_COLUMN_KEYS = [ 'address', 'provider', 'status', 'lastChecked' ] as const
type MailColumnKey = typeof MAIL_COLUMN_KEYS[number]

const columnLabelKeys = {
  address: 'table.address',
  provider: 'table.provider',
  status: 'table.status',
  lastChecked: 'table.lastChecked',
} as const satisfies Record<MailColumnKey, string>

// Starting widths in pixels, summing to less than the settings content column.
const columnSizes = {
  address: 320,
  provider: 170,
  status: 150,
  lastChecked: 220,
} as const satisfies Record<MailColumnKey, number>

export function MailAccountTable(props: Props) {
  const { t } = useTranslation([ 'email', 'common' ])
  const labels = useSmartTableLabels()
  const [ search, setSearch ] = useState('')

  const managedColumns = useMemo(() => {
    const renderCell = {
      // Its sender name and whether it is active are changed on its edit page, behind its Save.
      address: (account: MailAccount) => <div>
        <Link
          href={getMailboxEditUrl(account.id)}
          className='font-medium text-link no-underline hover:underline'
        >{
            account.address
          }</Link>
        {account.displayName && <div className='text-xs opacity-70'>{account.displayName}</div>}
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
      lastChecked: (account: MailAccount) => account.lastCheckedAt
        ? <DateText value={account.lastCheckedAt} isFocusable={false} />
        : t('table.never'),
    } satisfies Record<MailColumnKey, (account: MailAccount) => unknown>

    // Search matches what the user sees.
    const searchValue = {
      address: (account: MailAccount) => `${account.address} ${account.displayName}`,
      provider: (account: MailAccount) => t(mailAccountKindLabelKeys[account.kind]),
      status: (account: MailAccount) => t(mailAccountHealthLabelKeys[getMailAccountHealth(account)]),
      lastChecked: (account: MailAccount) => account.lastCheckedAt
        ? formatDateTime(account.lastCheckedAt)
        : t('table.never'),
    } satisfies Record<MailColumnKey, (account: MailAccount) => string>

    // Sorting follows the check's time, not its text, with mailboxes never checked first.
    const sortValue = {
      address: (account: MailAccount) => account.address,
      provider: (account: MailAccount) => t(mailAccountKindLabelKeys[account.kind]),
      status: (account: MailAccount) => t(mailAccountHealthLabelKeys[getMailAccountHealth(account)]),
      lastChecked: (account: MailAccount) => account.lastCheckedAt
        ? Date.parse(account.lastCheckedAt)
        : Number.NEGATIVE_INFINITY,
    } satisfies Record<MailColumnKey, (account: MailAccount) => string | number>

    return createManagedColumns<MailAccount, MailColumnKey>({
      columnKeys: MAIL_COLUMN_KEYS,
      getColumnLabel: (columnKey) => t(columnLabelKeys[columnKey]),
      getSearchKey: (columnKey) => columnKey,
      getSearchValue: (columnKey) => searchValue[columnKey],
      createColumnDef: ({ columnKey, columnId, columnLabel }) => ({
        id: columnId,
        header: columnLabel,
        accessorFn: sortValue[columnKey],
        size: columnSizes[columnKey],
        cell: ({ row }) => renderCell[columnKey](row.original),
      }),
    })
  }, [ t ])

  // Editing is the address's link, so it opens in a new tab too. A mailbox Elysium hosts is
  // deleted; one it signs in to is only disconnected.
  const rowActions = useMemo((): DataTableAction<MailAccount>[] => [
    {
      id: 'test',
      label: t('actions.test'),
      icon: <LuPlugZap className='size-4' aria-hidden />,
      onPress: (row) => props.onTest(row.original),
    },
    {
      id: 'sendTest',
      label: t('actions.sendTest'),
      icon: <LuSend className='size-4' aria-hidden />,
      onPress: (row) => props.onSendTest(row.original),
    },
    {
      id: 'disconnect',
      label: t('actions.disconnect'),
      icon: <LuUnplug className='size-4' aria-hidden />,
      isDanger: true,
      isVisible: (row) => row.original.kind !== 'self-hosted',
      onPress: (row) => props.onDisconnect(row.original),
    },
    {
      id: 'deleteMailbox',
      label: t('actions.deleteMailbox'),
      icon: <LuTrash2 className='size-4' aria-hidden />,
      isDanger: true,
      isVisible: (row) => row.original.kind === 'self-hosted',
      onPress: (row) => props.onDisconnect(row.original),
    },
  ], [ t, props.onTest, props.onSendTest, props.onDisconnect ])

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
    rowActions={rowActions}
    pagination={{ show: true }}
    enableSorting
    stickyHeader={false}
  />
}
