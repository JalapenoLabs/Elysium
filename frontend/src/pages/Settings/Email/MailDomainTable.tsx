// Copyright © 2026 Jalapeno Labs

import type { MailDomain } from '../../../api/routes/mailRoutes'
import type { MailDomainActions } from './MailDomainRowActions'

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Chip, Tooltip } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { MailDomainRowActions } from './MailDomainRowActions'

// Misc
import { useSmartTableLabels } from '../../../hooks/useSmartTableLabels'

type Props = MailDomainActions & {
  domains: MailDomain[]
  mailboxCounts: ReadonlyMap<string, number>
}

const DOMAIN_COLUMN_KEYS = [ 'name', 'mailboxes', 'added', 'rowActions' ] as const
type DomainColumnKey = typeof DOMAIN_COLUMN_KEYS[number]

const columnLabelKeys = {
  name: 'domains.table.name',
  mailboxes: 'domains.table.mailboxes',
  added: 'domains.table.added',
  rowActions: 'common:actions.moreActions',
} as const satisfies Record<DomainColumnKey, string>

// Starting widths in pixels, summing to less than the settings content column.
const columnSizes = {
  name: 360,
  mailboxes: 180,
  added: 220,
  rowActions: 64,
} as const satisfies Record<DomainColumnKey, number>

// A server has a handful of domains, so the table shows no search or column controls.
export function MailDomainTable(props: Props) {
  const { t, i18n } = useTranslation([ 'email', 'common' ])
  const labels = useSmartTableLabels()

  const managedColumns = useMemo(() => {
    // Timestamps arrive as UTC; this is the one place they become local time.
    const dateFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium' })
    const mailboxCountText = (domain: MailDomain) => t('domains.mailboxCount', {
      count: props.mailboxCounts.get(domain.id) ?? 0,
    })

    const renderCell = {
      name: (domain: MailDomain) => <span className='inline-flex items-center gap-2'>
        <span className='font-medium'>{domain.name}</span>
        {domain.isDefault && <Tooltip delay={200}>
          <Tooltip.Trigger>
            <Chip size='sm' variant='soft'>{t('domains.default')}</Chip>
          </Tooltip.Trigger>
          <Tooltip.Content className='max-w-xs'>
            <span>{t('domains.defaultHint')}</span>
          </Tooltip.Content>
        </Tooltip>}
      </span>,
      mailboxes: mailboxCountText,
      added: (domain: MailDomain) => dateFormatter.format(new Date(domain.createdAt)),
      rowActions: (domain: MailDomain) => <MailDomainRowActions
        domain={domain}
        onShowDns={props.onShowDns}
        onRemove={props.onRemove}
      />,
    } satisfies Record<DomainColumnKey, (domain: MailDomain) => unknown>

    const sortValue = {
      name: (domain: MailDomain) => domain.name,
      mailboxes: mailboxCountText,
      added: (domain: MailDomain) => domain.createdAt,
      rowActions: null,
    } satisfies Record<DomainColumnKey, ((domain: MailDomain) => string) | null>

    return createManagedColumns<MailDomain, DomainColumnKey>({
      columnKeys: DOMAIN_COLUMN_KEYS,
      getColumnLabel: (columnKey) => t(columnLabelKeys[columnKey]),
      getSearchKey: () => null,
      getSearchValue: (columnKey) => sortValue[columnKey],
      createColumnDef: ({ columnKey, columnId, columnLabel }) => {
        const toSortText = sortValue[columnKey]
        if (!toSortText) {
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
          accessorFn: toSortText,
          size: columnSizes[columnKey],
          cell: ({ row }) => renderCell[columnKey](row.original),
        }
      },
    })
  }, [ t, i18n.language, props.mailboxCounts, props.onShowDns, props.onRemove ])

  return <SmartTable
    ids={{
      tableElementId: 'mail-domains-table',
      tableLocalStorageId: 'elysium.settings.email.domains',
    }}
    tableAriaLabel={t('domains.table.label')}
    data={props.domains}
    managedColumns={managedColumns}
    getRowId={(domain) => domain.id}
    labels={labels}
    toolbar={{ show: false }}
    enableSorting
    stickyHeader={false}
  />
}
