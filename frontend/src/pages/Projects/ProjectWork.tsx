// Copyright © 2026 Jalapeno Labs

import type { ItemColumnKey } from '../ActionItems/ActionItemTable'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectProjectActionItems } from '../../store/actionItemsSlice'
import { selectProjectInitiatives } from '../../store/initiativesSlice'

// User interface
import { buttonVariants, Link, Spinner } from '@heroui/react'
import { LuPlus } from 'react-icons/lu'
import { EmptyNotice } from '../../components/EmptyNotice'
import { ActionItemTable } from '../ActionItems/ActionItemTable'
import { InitiativeTable } from '../Initiatives/InitiativeTable'

// Misc
import { useActionItemsLoader, useInitiativesLoader } from '../../hooks/useServerData'
import { useNow } from '../../hooks/useNow'
import { getNewActionItemUrl, getNewInitiativeUrl, UrlTree } from '../../urls'
import { writeActionItemFilters } from '../ActionItems/actionItemFilters'

type Props = {
  projectId: string
}

// Every item here is in this project.
const OMITTED_COLUMNS: readonly ItemColumnKey[] = [ 'projects' ]

// A project's work on its page: the items still owed (in the inbox or open) and the
// initiatives it holds, each with a way to add one here and a link to the full list.
export function ProjectWork(props: Props) {
  const { t } = useTranslation('projects')
  const now = useNow()
  const itemsStatus = useActionItemsLoader()
  const initiativesStatus = useInitiativesLoader()
  const projectItems = useAppSelector((state) => selectProjectActionItems(state, props.projectId))
  const initiatives = useAppSelector((state) => selectProjectInitiatives(state, props.projectId))

  const owedItems = projectItems.filter((item) => item.state === 'inbox' || item.state === 'open')
  const allItemsQuery = writeActionItemFilters({
    states: [],
    project: props.projectId,
    initiative: null,
    waiting: 'any',
    snoozed: 'any',
    deleted: false,
  })

  return <>
    <section className='relaxed'>
      <div className='level compact items-center'>
        <h2 className='text-xl font-semibold'>{t('work.itemsHeading')}</h2>
        <div className='level-right'>
          <Link href={`${UrlTree.actionItemsAll}?${allItemsQuery.toString()}`} className='text-sm text-link'>{
            t('work.seeAllItems', { count: projectItems.length })
          }</Link>
          <Link
            href={getNewActionItemUrl({ projectId: props.projectId })}
            className={buttonVariants({ size: 'sm', variant: 'outline', className: 'gap-2' })}
          >
            <LuPlus className='size-4' aria-hidden />
            <span>{t('work.newItem')}</span>
          </Link>
        </div>
      </div>
      {itemsStatus === 'loading' && <div className='grid place-items-center py-10'>
        <Spinner />
      </div>}
      {itemsStatus !== 'loading' && !owedItems.length && <EmptyNotice>{
        t('work.itemsEmpty')
      }</EmptyNotice>}
      {owedItems.length > 0 && <ActionItemTable
        items={owedItems}
        now={now}
        label={t('work.itemsHeading')}
        storageId='elysium.projects.items.table.v1'
        omittedColumns={OMITTED_COLUMNS}
      />}
    </section>

    <section className='relaxed'>
      <div className='level compact items-center'>
        <h2 className='text-xl font-semibold'>{t('work.initiativesHeading')}</h2>
        <Link
          href={getNewInitiativeUrl(props.projectId)}
          className={buttonVariants({ size: 'sm', variant: 'outline', className: 'gap-2' })}
        >
          <LuPlus className='size-4' aria-hidden />
          <span>{t('work.newInitiative')}</span>
        </Link>
      </div>
      {initiativesStatus === 'loading' && <div className='grid place-items-center py-10'>
        <Spinner />
      </div>}
      {initiativesStatus !== 'loading' && !initiatives.length && <EmptyNotice>{
        t('work.initiativesEmpty')
      }</EmptyNotice>}
      {initiatives.length > 0 && <InitiativeTable
        initiatives={initiatives}
        label={t('work.initiativesHeading')}
        storageId='elysium.projects.initiatives.table.v1'
      />}
    </section>
  </>
}
