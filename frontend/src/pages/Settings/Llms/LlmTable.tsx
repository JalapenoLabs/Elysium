// Copyright © 2026 Jalapeno Labs

import type { DataTableAction } from '@jalapenolabs/uikit'
import type { Llm } from '../../../api/routes/llmRoutes'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Chip, Link } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { LuPower, LuPowerOff, LuTrash2 } from 'react-icons/lu'
import { ClaudeUsageWindow } from './ClaudeUsageWindow'

// Misc
import { formatDate } from '../../../format/dates'
import { formatPercent } from '../../../format/numbers'
import { useSmartTableLabels } from '../../../hooks/useSmartTableLabels'
import { useUserNames } from '../../../hooks/useUserNames'
import { getLlmEditUrl } from '../../../urls'
import {
  getLlmStatus,
  llmStatusChipColors,
  llmStatusLabelKeys,
  llmTypeLabelKeys,
} from './llmPresentation'

type Props = {
  llms: Llm[]
  onToggleActive: (llm: Llm) => Promise<void>
  onDelete: (llm: Llm) => void
}

const LLM_COLUMN_KEYS = [
  'name',
  'account',
  'type',
  'priority',
  'status',
  'fiveHour',
  'sevenDay',
  'expiresAt',
  'createdBy',
] as const
type LlmColumnKey = typeof LLM_COLUMN_KEYS[number]

const columnLabelKeys = {
  name: 'table.name',
  account: 'table.account',
  type: 'table.type',
  priority: 'table.priority',
  status: 'table.status',
  fiveHour: 'table.fiveHourUsage',
  sevenDay: 'table.sevenDayUsage',
  expiresAt: 'table.expires',
  createdBy: 'common:table.createdBy',
} as const satisfies Record<LlmColumnKey, string>

// Starting widths in pixels.
const columnSizes = {
  name: 240,
  account: 220,
  type: 180,
  priority: 110,
  status: 130,
  fiveHour: 210,
  sevenDay: 210,
  expiresAt: 160,
  createdBy: 160,
} as const satisfies Record<LlmColumnKey, number>

export function LlmTable(props: Props) {
  const { t, i18n } = useTranslation([ 'llms', 'common' ])
  const labels = useSmartTableLabels()
  const userNames = useUserNames()
  const [ search, setSearch ] = useState('')

  // Captured once per mount so render stays pure; status is "as of when you opened the page".
  const [ now ] = useState(() => Date.now())

  const managedColumns = useMemo(() => {
    // Expiry is chosen as a date, so the time of day would only be noise.
    // A signed-in account never expires while Elysium keeps it signed in.
    function expiryText(llm: Llm) {
      if (llm.account) {
        return t('table.keptSignedIn')
      }
      if (!llm.expiresAt) {
        return t('table.never')
      }
      return formatDate(llm.expiresAt)
    }

    // The share of a window used, once Anthropic has reported it.
    function utilization(llm: Llm, window: 'fiveHour' | 'sevenDay') {
      const usage = llm.account?.usage
      if (!usage?.readAt) {
        return null
      }
      return usage[window]?.utilization ?? null
    }

    function usageText(llm: Llm, window: 'fiveHour' | 'sevenDay') {
      const used = utilization(llm, window)
      if (used === null) {
        return t('usage.notAvailable')
      }
      return formatPercent(used / 100)
    }

    const renderCell = {
      name: (llm: Llm) => <div>
        <Link href={getLlmEditUrl(llm.id)} className='font-medium text-link no-underline hover:underline'>{
          llm.name
        }</Link>
        {llm.description && <div className='text-xs opacity-70'>{llm.description}</div>}
      </div>,
      account: (llm: Llm) => llm.account?.email ?? '',
      type: (llm: Llm) => t(llmTypeLabelKeys[llm.type]),
      priority: (llm: Llm) => llm.priority,
      status: (llm: Llm) => {
        const status = getLlmStatus(llm, now)
        return <Chip size='sm' variant='soft' color={llmStatusChipColors[status]}>{
          t(llmStatusLabelKeys[status])
        }</Chip>
      },
      fiveHour: (llm: Llm) => <ClaudeUsageWindow
        account={llm.account}
        window='fiveHour'
        label={t('table.fiveHourUsage')}
      />,
      sevenDay: (llm: Llm) => <ClaudeUsageWindow
        account={llm.account}
        window='sevenDay'
        label={t('table.sevenDayUsage')}
      />,
      expiresAt: expiryText,
      createdBy: (llm: Llm) => userNames[llm.createdBy] ?? t('common:table.someone'),
    } satisfies Record<LlmColumnKey, (llm: Llm) => unknown>

    // Search matches what the user sees, not raw enum values or timestamps.
    const searchValue = {
      name: (llm: Llm) => `${llm.name} ${llm.description}`,
      account: (llm: Llm) => llm.account?.email ?? '',
      type: (llm: Llm) => t(llmTypeLabelKeys[llm.type]),
      priority: (llm: Llm) => String(llm.priority),
      status: (llm: Llm) => t(llmStatusLabelKeys[getLlmStatus(llm, now)]),
      fiveHour: (llm: Llm) => usageText(llm, 'fiveHour'),
      sevenDay: (llm: Llm) => usageText(llm, 'sevenDay'),
      expiresAt: expiryText,
      createdBy: (llm: Llm) => userNames[llm.createdBy] ?? '',
    } satisfies Record<LlmColumnKey, (llm: Llm) => string>

    // Sorting follows values, not their text: priorities and usage as numbers (an unread window
    // below an empty one), and expiry in time, with what never expires last.
    const sortValue = {
      name: (llm: Llm) => llm.name.toLocaleLowerCase(i18n.language),
      account: (llm: Llm) => llm.account?.email ?? '',
      type: (llm: Llm) => t(llmTypeLabelKeys[llm.type]),
      priority: (llm: Llm) => llm.priority,
      status: (llm: Llm) => t(llmStatusLabelKeys[getLlmStatus(llm, now)]),
      fiveHour: (llm: Llm) => utilization(llm, 'fiveHour') ?? -1,
      sevenDay: (llm: Llm) => utilization(llm, 'sevenDay') ?? -1,
      expiresAt: (llm: Llm) => llm.expiresAt && !llm.account
        ? Date.parse(llm.expiresAt)
        : Number.POSITIVE_INFINITY,
      createdBy: (llm: Llm) => userNames[llm.createdBy] ?? '',
    } satisfies Record<LlmColumnKey, (llm: Llm) => string | number>

    return createManagedColumns<Llm, LlmColumnKey>({
      columnKeys: LLM_COLUMN_KEYS,
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
  }, [ t, i18n.language, userNames, now ])

  // Editing is the name's link, so it opens in a new tab too.
  const rowActions = useMemo((): DataTableAction<Llm>[] => [
    {
      id: 'activate',
      label: t('actions.activate'),
      icon: <LuPower className='size-4' aria-hidden />,
      isVisible: (row) => !row.original.isActive,
      onPress: (row) => props.onToggleActive(row.original),
    },
    {
      id: 'deactivate',
      label: t('actions.deactivate'),
      icon: <LuPowerOff className='size-4' aria-hidden />,
      isVisible: (row) => row.original.isActive,
      onPress: (row) => props.onToggleActive(row.original),
    },
    {
      id: 'delete',
      label: t('common:actions.delete'),
      icon: <LuTrash2 className='size-4' aria-hidden />,
      isDanger: true,
      onPress: (row) => props.onDelete(row.original),
    },
  ], [ t, props.onToggleActive, props.onDelete ])

  if (!props.llms.length) {
    return <p className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'>{
      t('table.empty')
    }</p>
  }

  return <SmartTable
    ids={{
      tableElementId: 'llm-credentials-table',
      tableLocalStorageId: 'elysium.settings.llms.table.v4',
    }}
    tableAriaLabel={t('table.label')}
    data={props.llms}
    managedColumns={managedColumns}
    getRowId={(llm) => llm.id}
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
