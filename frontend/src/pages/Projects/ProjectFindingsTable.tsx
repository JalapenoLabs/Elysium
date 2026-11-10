// Copyright © 2026 Jalapeno Labs

import type { DataTableAction } from '@jalapenolabs/uikit'
import type { Finding } from '../../api/routes/findingRoutes'

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Chip, Link } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { LuCircleCheck, LuCircleX, LuExternalLink } from 'react-icons/lu'

// Misc
import { DateText } from '../../format/DateText'
import { formatNumber } from '../../format/numbers'
import { useSmartTableLabels } from '../../hooks/useSmartTableLabels'
import {
  findingCostChipColors,
  findingCostLabelKeys,
  findingCostOrder,
  repositoryHref,
} from './findingPresentation'
import { useFindingActions } from './useFindingActions'

type Props = {
  findings: Finding[]
}

const FINDING_COLUMN_KEYS = [ 'title', 'detail', 'place', 'cost', 'reports', 'reported' ] as const
type FindingColumnKey = typeof FINDING_COLUMN_KEYS[number]

const columnLabelKeys = {
  title: 'findings.columns.title',
  detail: 'findings.columns.detail',
  place: 'findings.columns.place',
  cost: 'findings.columns.cost',
  reports: 'findings.columns.reports',
  reported: 'findings.columns.reported',
} as const satisfies Record<FindingColumnKey, string>

// Starting widths in pixels, summing to less than the page's content column.
const columnSizes = {
  title: 190,
  detail: 290,
  place: 200,
  cost: 90,
  reports: 70,
  reported: 120,
} as const satisfies Record<FindingColumnKey, number>

// A project's open findings, newest first: what an agent noticed, what it saw, where, what it
// would cost, and how often it was seen, each accepted or dismissed from its row. A long detail
// is clamped, and clicking it opens its row in full.
export function ProjectFindingsTable(props: Props) {
  const { t } = useTranslation('projects')
  const labels = useSmartTableLabels()
  const act = useFindingActions()

  const managedColumns = useMemo(() => {
    function costLabel(finding: Finding) {
      return t(findingCostLabelKeys[finding.cost])
    }

    const renderCell = {
      title: (finding: Finding) => <span className='font-medium'>{finding.title}</span>,
      detail: (finding: Finding) => <span className='text-sm whitespace-pre-line opacity-80'>{
        finding.detail
      }</span>,
      // The place first, then the repository it is in, smaller beneath it.
      place: (finding: Finding) => {
        const href = repositoryHref(finding)
        return <div className='flex min-w-0 flex-col items-start gap-1'>
          <span className='text-sm break-all'>{finding.location}</span>
          {href && <Link
            href={href}
            target='_blank'
            rel='noreferrer'
            className='text-xs text-link'
          >
            <span className='break-all'>{href}</span>
            <LuExternalLink className='size-3 shrink-0' aria-hidden />
          </Link>}
          {!href && finding.repositoryUrl && <span className='text-xs break-all opacity-70'>{
            finding.repositoryUrl
          }</span>}
        </div>
      },
      cost: (finding: Finding) => <Chip size='sm' variant='soft' color={findingCostChipColors[finding.cost]}>{
        costLabel(finding)
      }</Chip>,
      reports: (finding: Finding) => <span className='tabular-nums opacity-80'>{
        formatNumber(finding.reports)
      }</span>,
      reported: (finding: Finding) => <DateText value={finding.createdAt} isFocusable={false} />,
    } satisfies Record<FindingColumnKey, (finding: Finding) => unknown>

    // Each column's value, which is also what the export holds: the place with its repository,
    // and the instant it was first reported.
    const searchValue = {
      title: (finding: Finding) => finding.title,
      detail: (finding: Finding) => finding.detail,
      place: (finding: Finding) => [ finding.location, finding.repositoryUrl ].filter(Boolean).join(' '),
      cost: costLabel,
      reports: (finding: Finding) => String(finding.reports),
      reported: (finding: Finding) => finding.createdAt,
    } satisfies Record<FindingColumnKey, (finding: Finding) => string>

    // Cost sorts small to large and reports as a number; the rest by their values.
    const sortValue = {
      ...searchValue,
      cost: (finding: Finding) => findingCostOrder[finding.cost],
      reports: (finding: Finding) => finding.reports,
    } satisfies Record<FindingColumnKey, (finding: Finding) => string | number>

    return createManagedColumns<Finding, FindingColumnKey>({
      columnKeys: FINDING_COLUMN_KEYS,
      getColumnLabel: (columnKey) => t(columnLabelKeys[columnKey]),
      getSearchKey: () => null,
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

  const rowActions = useMemo((): DataTableAction<Finding>[] => [
    {
      id: 'accept',
      label: t('findings.actions.accept'),
      icon: <LuCircleCheck className='size-4' aria-hidden />,
      onPress: (row) => act.accept(row.original),
    },
    {
      id: 'dismiss',
      label: t('findings.actions.dismiss'),
      icon: <LuCircleX className='size-4' aria-hidden />,
      isDanger: true,
      onPress: (row) => act.dismiss(row.original),
    },
  ], [ t, act ])

  return <SmartTable
    ids={{
      tableElementId: 'project-findings-table',
      tableLocalStorageId: 'elysium.projects.findings.table.v1',
    }}
    tableAriaLabel={t('findings.label')}
    data={props.findings}
    managedColumns={managedColumns}
    getRowId={(finding) => finding.id}
    labels={labels}
    rowActions={rowActions}
    toolbar={{ show: false }}
    csvExport={{ fileName: 'findings' }}
    pagination={{ show: true }}
    enableSorting
    cellOverflow='expandRow'
    stickyHeader={false}
  />
}
