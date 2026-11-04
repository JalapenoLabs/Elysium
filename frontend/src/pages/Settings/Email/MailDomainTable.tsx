// Copyright © 2026 Jalapeno Labs

import type { DataTableAction } from '@jalapenolabs/uikit'
import type { MailDomain } from '../../../api/routes/mailRoutes'

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Chip, Tooltip } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { LuListTree, LuTrash2 } from 'react-icons/lu'

// Misc
import { DateText } from '../../../format/DateText'
import { useSmartTableLabels } from '../../../hooks/useSmartTableLabels'

type Props = {
  domains: MailDomain[]
  mailboxCounts: ReadonlyMap<string, number>
  onShowDns: (domain: MailDomain) => void
  onRemove: (domain: MailDomain) => void
}

const DOMAIN_COLUMN_KEYS = [ 'name', 'isDefault', 'mailboxes', 'added' ] as const
type DomainColumnKey = typeof DOMAIN_COLUMN_KEYS[number]

const columnLabelKeys = {
  name: 'domains.table.name',
  isDefault: 'domains.default',
  mailboxes: 'domains.table.mailboxes',
  added: 'domains.table.added',
} as const satisfies Record<DomainColumnKey, string>

// Starting widths in pixels, summing to less than the settings content column.
const columnSizes = {
  name: 320,
  isDefault: 140,
  mailboxes: 180,
  added: 200,
} as const satisfies Record<DomainColumnKey, number>

// A server has a handful of domains, so the table shows no search or column controls.
export function MailDomainTable(props: Props) {
  const { t } = useTranslation([ 'email', 'common' ])
  const labels = useSmartTableLabels()

  const managedColumns = useMemo(() => {
    const mailboxCount = (domain: MailDomain) => props.mailboxCounts.get(domain.id) ?? 0

    const renderCell = {
      name: (domain: MailDomain) => <span className='font-medium'>{domain.name}</span>,
      isDefault: (domain: MailDomain) => domain.isDefault && <Tooltip delay={200}>
        <Tooltip.Trigger>
          <Chip size='sm' variant='soft'>{t('domains.default')}</Chip>
        </Tooltip.Trigger>
        <Tooltip.Content className='max-w-xs'>
          <span>{t('domains.defaultHint')}</span>
        </Tooltip.Content>
      </Tooltip>,
      mailboxes: (domain: MailDomain) => t('domains.mailboxCount', { count: mailboxCount(domain) }),
      added: (domain: MailDomain) => <DateText value={domain.createdAt} isFocusable={false} />,
    } satisfies Record<DomainColumnKey, (domain: MailDomain) => unknown>

    // Counts and dates sort by their values, not their text; the default domain sorts first.
    const sortValue = {
      name: (domain: MailDomain) => domain.name,
      isDefault: (domain: MailDomain) => domain.isDefault
        ? 0
        : 1,
      mailboxes: mailboxCount,
      added: (domain: MailDomain) => Date.parse(domain.createdAt),
    } satisfies Record<DomainColumnKey, (domain: MailDomain) => string | number>

    return createManagedColumns<MailDomain, DomainColumnKey>({
      columnKeys: DOMAIN_COLUMN_KEYS,
      getColumnLabel: (columnKey) => t(columnLabelKeys[columnKey]),
      // The table has no search field.
      getSearchKey: () => null,
      getSearchValue: () => null,
      createColumnDef: ({ columnKey, columnId, columnLabel }) => ({
        id: columnId,
        header: columnLabel,
        accessorFn: sortValue[columnKey],
        size: columnSizes[columnKey],
        cell: ({ row }) => renderCell[columnKey](row.original),
      }),
    })
  }, [ t, props.mailboxCounts ])

  // The default domain cannot be removed; its Default chip says why.
  const rowActions = useMemo((): DataTableAction<MailDomain>[] => [
    {
      id: 'dns',
      label: t('domains.actions.dns'),
      icon: <LuListTree className='size-4' aria-hidden />,
      onPress: (row) => props.onShowDns(row.original),
    },
    {
      id: 'remove',
      label: t('domains.actions.remove'),
      icon: <LuTrash2 className='size-4' aria-hidden />,
      isDanger: true,
      isDisabled: (row) => row.original.isDefault,
      onPress: (row) => props.onRemove(row.original),
    },
  ], [ t, props.onShowDns, props.onRemove ])

  return <SmartTable
    ids={{
      tableElementId: 'mail-domains-table',
      tableLocalStorageId: 'elysium.settings.email.domains.v2',
    }}
    tableAriaLabel={t('domains.table.label')}
    data={props.domains}
    managedColumns={managedColumns}
    getRowId={(domain) => domain.id}
    labels={labels}
    toolbar={{ show: false }}
    rowActions={rowActions}
    pagination={{ show: true }}
    enableSorting
    stickyHeader={false}
  />
}
