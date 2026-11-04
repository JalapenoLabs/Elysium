// Copyright © 2026 Jalapeno Labs

import type { ActionItem } from '../../api/routes/actionItemRoutes'
import type { LoadStatus } from '../../hooks/useServerData'
import type { ItemColumnKey } from '../ActionItems/ActionItemTable'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { buttonVariants, Link, Spinner } from '@heroui/react'
import { LuPlus } from 'react-icons/lu'
import { EmptyNotice } from '../../components/EmptyNotice'
import { ActionItemTable } from '../ActionItems/ActionItemTable'

// Misc
import { useNow } from '../../hooks/useNow'
import { getNewActionItemUrl, UrlTree } from '../../urls'
import { writeActionItemFilters } from '../ActionItems/actionItemFilters'

type Props = {
  projectId: string
  status: LoadStatus
  // The project's items still owed: in the inbox or open.
  owedItems: ActionItem[]
  // Every live item of the project, resolved and dismissed included, which All items lists.
  itemCount: number
}

// Every item here is in this project.
const OMITTED_COLUMNS: readonly ItemColumnKey[] = [ 'projects' ]

// A project's Action items tab: the items still owed, a way to add one here, and a link to
// every item of the project in All items.
export function ProjectItems(props: Props) {
  const { t } = useTranslation('projects')
  const now = useNow()

  const allItemsQuery = writeActionItemFilters({
    states: [],
    project: props.projectId,
    initiative: null,
    waiting: 'any',
    snoozed: 'any',
    deleted: false,
  })

  return <>
    <div className='level-right compact justify-end'>
      <Link href={`${UrlTree.actionItemsAll}?${allItemsQuery.toString()}`} className='text-sm text-link'>{
        t('work.seeAllItems', { count: props.itemCount })
      }</Link>
      <Link
        href={getNewActionItemUrl({ projectId: props.projectId })}
        className={buttonVariants({ size: 'sm', variant: 'outline', className: 'gap-2' })}
      >
        <LuPlus className='size-4' aria-hidden />
        <span>{t('work.newItem')}</span>
      </Link>
    </div>
    {props.status === 'loading' && <div className='grid place-items-center py-10'>
      <Spinner />
    </div>}
    {props.status !== 'loading' && !props.owedItems.length && <EmptyNotice>{
      t('work.itemsEmpty')
    }</EmptyNotice>}
    {props.owedItems.length > 0 && <ActionItemTable
      items={props.owedItems}
      now={now}
      label={t('work.itemsHeading')}
      storageId='elysium.projects.items.table.v1'
      omittedColumns={OMITTED_COLUMNS}
    />}
  </>
}
