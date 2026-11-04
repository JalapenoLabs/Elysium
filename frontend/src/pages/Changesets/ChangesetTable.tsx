// Copyright © 2026 Jalapeno Labs

import type { CsvValue } from '@jalapenolabs/uikit'
import type { ReactNode } from 'react'
import type { Changeset } from '../../api/routes/changesetRoutes'

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'
import { shallowEqual } from 'react-redux'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectProjectNamesById } from '../../store/projectsSlice'

// User interface
import { Chip, Link } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'

// Misc
import { CHANGESET_STATES } from '../../api/routes/changesetRoutes'
import { DateText } from '../../format/DateText'
import { formatDateTime } from '../../format/dates'
import { useProjectsLoader } from '../../hooks/useServerData'
import { useSmartTableLabels } from '../../hooks/useSmartTableLabels'
import { useUserNames } from '../../hooks/useUserNames'
import { getChangesetViewUrl } from '../../urls'
import { describeActor } from '../ActionItems/actionItemPresentation'
import {
  appliedWithoutReview,
  changesetStateChipColors,
  changesetStateLabelKeys,
  tallyDecisions,
} from './changesetPresentation'

// The Changesets page's two lists, one per tab.
export type ChangesetList = 'pending' | 'reviewed'

type Props = {
  changesets: Changeset[]
  list: ChangesetList
  // The page's titlebar, where the table renders its search, Columns, and export.
  controlsContainer: Element | null
  // Said in place of the rows when there are none. The table itself stays, so its controls
  // stay in the titlebar too.
  emptyNotice: ReactNode
}

type ChangesetColumnKey = 'summary' | 'state' | 'proposer' | 'project' | 'changes' | 'proposed' | 'decided'

// Every changeset waiting for review is in the same state and undecided, so that list leaves out
// both columns and counts its decisions so far under its changes instead. A reviewed one is placed
// in time by its decision; when it was proposed is on its review.
const columnKeysByList = {
  pending: [ 'summary', 'proposer', 'project', 'changes', 'proposed' ],
  reviewed: [ 'summary', 'state', 'proposer', 'project', 'changes', 'decided' ],
} as const satisfies Record<ChangesetList, readonly ChangesetColumnKey[]>

const columnLabelKeys = {
  summary: 'table.summary',
  state: 'table.state',
  proposer: 'table.proposer',
  project: 'table.project',
  changes: 'table.changes',
  proposed: 'table.proposed',
  decided: 'table.decided',
} as const satisfies Record<ChangesetColumnKey, string>

// Starting widths in pixels, each list summing to less than the content column at md. The
// changes column is wide only where it counts decisions.
const columnSizesByList = {
  pending: { summary: 290, proposer: 140, project: 150, changes: 230, proposed: 130 },
  reviewed: { summary: 280, state: 130, proposer: 140, project: 150, changes: 110, decided: 140 },
} as const satisfies { [List in ChangesetList]: Record<typeof columnKeysByList[List][number], number> }

// uikit holds a column to 160 pixels unless it sets its own floor, and most of these need less.
const COLUMN_MIN_SIZE = 100

const ariaLabelKeysByList = {
  pending: 'tabs.pending',
  reviewed: 'tabs.reviewed',
} as const satisfies Record<ChangesetList, string>

// Each list remembers its own column widths and order.
const storageIdsByList = {
  pending: 'elysium.changesets.pending.table.v1',
  reviewed: 'elysium.changesets.reviewed.table.v1',
} as const satisfies Record<ChangesetList, string>

const exportFileNamesByList = {
  pending: 'changesets-waiting-for-review',
  reviewed: 'changesets-reviewed',
} as const satisfies Record<ChangesetList, string>

// Changesets in a table, newest first until sorted otherwise. Clicking a row opens its review.
export function ChangesetTable(props: Props) {
  const { t, i18n } = useTranslation([ 'changesets', 'actionItems', 'common' ])
  const navigate = useNavigate()
  const labels = useSmartTableLabels()
  const userNames = useUserNames()
  useProjectsLoader()
  const projectNames = useAppSelector(selectProjectNamesById, shallowEqual)
  const hasChangesets = props.changesets.length > 0

  const managedColumns = useMemo(() => {
    const columnSizes: Partial<Record<ChangesetColumnKey, number>> = columnSizesByList[props.list]

    function proposerText(changeset: Changeset) {
      const proposer = describeActor(changeset.proposer)
      return t(proposer.key, { ...proposer.values, ns: 'actionItems' })
    }
    // A changeset outside any project, or in one since deleted, names none.
    function projectText(changeset: Changeset) {
      if (!changeset.projectId) {
        return ''
      }
      return projectNames[changeset.projectId] ?? ''
    }
    function countText(changeset: Changeset) {
      return t('changeCount', { count: changeset.operations.length })
    }
    // Only a changeset still waiting has decisions worth counting: once reviewed, every change
    // was decided along with it.
    function tallyText(changeset: Changeset) {
      if (changeset.state !== 'pending') {
        return ''
      }
      return t('review.tally', tallyDecisions(changeset))
    }
    function deciderText(changeset: Changeset) {
      if (!changeset.decidedBy) {
        return ''
      }
      if (appliedWithoutReview(changeset)) {
        return t('review.withoutReview')
      }
      return userNames[changeset.decidedBy] ?? t('common:table.someone')
    }
    function decidedText(changeset: Changeset) {
      if (!changeset.decidedAt) {
        return ''
      }
      return `${formatDateTime(changeset.decidedAt)} ${deciderText(changeset)}`
    }
    function stateText(changeset: Changeset) {
      return t(changesetStateLabelKeys[changeset.state])
    }

    const renderCell = {
      summary: (changeset: Changeset) => <Link
        href={getChangesetViewUrl(changeset.id)}
        className='font-medium text-link no-underline hover:underline'
      >{
        changeset.summary
      }</Link>,
      state: (changeset: Changeset) => <Chip
        size='sm'
        variant='soft'
        color={changesetStateChipColors[changeset.state]}
      >{
        stateText(changeset)
      }</Chip>,
      proposer: proposerText,
      project: projectText,
      changes: (changeset: Changeset) => <div>
        <div>{countText(changeset)}</div>
        {changeset.state === 'pending' && <div className='text-xs opacity-70'>{tallyText(changeset)}</div>}
      </div>,
      proposed: (changeset: Changeset) => <DateText value={changeset.createdAt} isFocusable={false} />,
      decided: (changeset: Changeset) => changeset.decidedAt && <div>
        <DateText value={changeset.decidedAt} isFocusable={false} />
        <div className='text-xs opacity-70'>{deciderText(changeset)}</div>
      </div>,
    } satisfies Record<ChangesetColumnKey, (changeset: Changeset) => unknown>

    // Search matches what the user sees.
    const searchValue = {
      summary: (changeset: Changeset) => changeset.summary,
      state: stateText,
      proposer: proposerText,
      project: projectText,
      changes: (changeset: Changeset) => `${countText(changeset)} ${tallyText(changeset)}`,
      proposed: (changeset: Changeset) => formatDateTime(changeset.createdAt),
      decided: decidedText,
    } satisfies Record<ChangesetColumnKey, (changeset: Changeset) => string>

    // Words sort without regard to case, changes by how many, the state through its lifecycle,
    // and dates in time, with one not yet decided last.
    const sortValue = {
      summary: (changeset: Changeset) => changeset.summary.toLocaleLowerCase(i18n.language),
      state: (changeset: Changeset) => CHANGESET_STATES.indexOf(changeset.state),
      proposer: (changeset: Changeset) => proposerText(changeset).toLocaleLowerCase(i18n.language),
      project: (changeset: Changeset) => projectText(changeset).toLocaleLowerCase(i18n.language),
      changes: (changeset: Changeset) => changeset.operations.length,
      proposed: (changeset: Changeset) => Date.parse(changeset.createdAt),
      decided: (changeset: Changeset) => changeset.decidedAt
        ? Date.parse(changeset.decidedAt)
        : Number.POSITIVE_INFINITY,
    } satisfies Record<ChangesetColumnKey, (changeset: Changeset) => string | number>

    // The export holds the state by name, the number of changes, and the instants themselves.
    const exportValue = {
      summary: (changeset: Changeset) => changeset.summary,
      state: stateText,
      proposer: proposerText,
      project: projectText,
      changes: (changeset: Changeset) => changeset.operations.length,
      proposed: (changeset: Changeset) => changeset.createdAt,
      decided: (changeset: Changeset) => changeset.decidedAt,
    } satisfies Record<ChangesetColumnKey, (changeset: Changeset) => CsvValue>

    return createManagedColumns<Changeset, ChangesetColumnKey>({
      columnKeys: columnKeysByList[props.list],
      getColumnLabel: (columnKey) => t(columnLabelKeys[columnKey]),
      getSearchKey: (columnKey) => columnKey,
      getSearchValue: (columnKey) => searchValue[columnKey],
      getExportValue: (columnKey) => exportValue[columnKey],
      createColumnDef: ({ columnKey, columnId, columnLabel }) => ({
        id: columnId,
        header: columnLabel,
        accessorFn: sortValue[columnKey],
        size: columnSizes[columnKey],
        minSize: COLUMN_MIN_SIZE,
        cell: ({ row }) => renderCell[columnKey](row.original),
      }),
    })
  }, [ t, i18n.language, props.list, projectNames, userNames ])

  return <>
    <SmartTable
      ids={{
        tableElementId: storageIdsByList[props.list],
        tableLocalStorageId: storageIdsByList[props.list],
      }}
      className={hasChangesets
        ? '[&_tbody_tr]:cursor-pointer'
        : 'hidden'}
      tableAriaLabel={t(ariaLabelKeysByList[props.list])}
      data={props.changesets}
      managedColumns={managedColumns}
      getRowId={(changeset) => changeset.id}
      labels={labels}
      controls={{ container: props.controlsContainer }}
      csvExport={{ fileName: exportFileNamesByList[props.list] }}
      pagination={{ show: true }}
      enableSorting
      // A row click opens the review; nothing is ever shown as highlighted. Since a click goes
      // somewhere, a clamped cell opens in full on a double-click instead of expanding the row.
      cellOverflow='modal'
      enableRowHighlight
      highlightedRowId={null}
      onHighlightedRowChange={(changesetId) => {
        if (changesetId) {
          navigate(getChangesetViewUrl(changesetId))
        }
      }}
      stickyHeader={false}
    />
    {!hasChangesets && props.emptyNotice}
  </>
}
