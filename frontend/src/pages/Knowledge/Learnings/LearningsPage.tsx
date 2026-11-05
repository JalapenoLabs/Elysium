// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'

// Core
import { useSearchParams } from 'react-router'
import { useTranslation } from 'react-i18next'

// Redux
import { selectIsAdmin } from '../../../store/authSlice'
import { useAppSelector } from '../../../store/hooks'
import { selectDeletedLearnings, selectLiveLearnings } from '../../../store/learningsSlice'

// User interface
import { buttonVariants, Link, Spinner, Tabs } from '@heroui/react'
import { LuPlus } from 'react-icons/lu'
import { EmptyNotice } from '../../../components/EmptyNotice'
import { OptionSelect } from '../../../components/OptionSelect'
import { PageTabs } from '../../../components/PageTabs'
import { Page } from '../../../layout/page/Page'
import { LearningFeed } from './LearningFeed'
import { LearningTable } from './LearningTable'

// Misc
import { useLearningCategories } from '../../../hooks/useLearningCategories'
import { useDeletedLearningsLoader, useLearningsLoader } from '../../../hooks/useServerData'
import { useTabParam } from '../../../hooks/useTabParam'
import { UrlTree } from '../../../urls'

const TABS = [ 'live', 'deleted', 'feed' ] as const
type Tab = typeof TABS[number]

const tabLabelKeys = {
  live: 'tabs.live',
  deleted: 'tabs.deleted',
  feed: 'tabs.feed',
} as const satisfies Record<Tab, ParseKeys<'learnings'>>

// The category filter lives in the address, as the open tab does, so coming back from a
// learning returns to both.
const CATEGORY_PARAM = 'category'

// The filter's choice for every category, as the select's key; the address then names none.
const ANY_CATEGORY = 'any'

// `/knowledge/learnings`: every learning, the deleted ones, and, for admins, the feed of every
// change, each on a tab of its own. Both lists filter by category, built-in or the workspace's own.
export function LearningsPage() {
  const { t } = useTranslation([ 'learnings', 'knowledge', 'common' ])
  const isAdmin = useAppSelector(selectIsAdmin)
  const [ params, setParams ] = useSearchParams()
  const shownTabs = TABS.filter((tab) => tab !== 'feed' || isAdmin)
  const [ selectedTab, selectTab ] = useTabParam(shownTabs)

  const liveStatus = useLearningsLoader()
  // Only the company's members reach this page, inside the knowledge base's layout.
  const deletedStatus = useDeletedLearningsLoader(true)
  const live = useAppSelector(selectLiveLearnings)
  const deleted = useAppSelector(selectDeletedLearnings)
  const { categories } = useLearningCategories()
  const categoryFilter = params.get(CATEGORY_PARAM)

  function filterByCategory(value: string) {
    setParams((current) => {
      const next = new URLSearchParams(current)
      if (value === ANY_CATEGORY) {
        next.delete(CATEGORY_PARAM)
        return next
      }
      next.set(CATEGORY_PARAM, value)
      return next
    }, { replace: true })
  }

  // Lists show their totals beside their names, once they are known.
  const countByTab: Record<Tab, number | null> = {
    live: liveStatus === 'loaded'
      ? live.length
      : null,
    deleted: deletedStatus === 'loaded'
      ? deleted.length
      : null,
    feed: null,
  }

  function renderList(status: typeof liveStatus, isDeletedList: boolean) {
    if (status === 'loading') {
      return <div className='grid place-items-center py-16'>
        <Spinner />
      </div>
    }
    if (status === 'failed') {
      return <p className='py-10 text-center text-sm text-danger'>{t('table.loadError')}</p>
    }
    const learnings = isDeletedList
      ? deleted
      : live
    const shown = categoryFilter
      ? learnings.filter((learning) => learning.category === categoryFilter)
      : learnings
    const categoryOptions = [
      { id: ANY_CATEGORY, label: t('filters.anyCategory') },
      ...categories.map((category) => ({ id: category.key, label: category.label })),
    ]

    return <>
      <div className='relaxed'>
        <OptionSelect
          id='learnings-category-filter'
          className='w-56'
          label={t('filters.category')}
          options={categoryOptions}
          value={categoryFilter ?? ANY_CATEGORY}
          onChange={filterByCategory}
        />
      </div>
      {learnings.length > 0 && !shown.length
        ? <EmptyNotice>{t('filters.noMatches')}</EmptyNotice>
        : <LearningTable
          learnings={shown}
          isDeletedList={isDeletedList}
        />}
    </>
  }

  return <Page
    title={t('title')}
    parents={[{ label: t('knowledge:title'), href: UrlTree.knowledge }]}
    primaryAction={<Link
      href={UrlTree.knowledgeLearningNew}
      className={buttonVariants({ variant: 'outline' })}
    >
      <LuPlus className='size-4' aria-hidden />
      <span>{t('new')}</span>
    </Link>}
  >
    <PageTabs
      label={t('tabs.label')}
      selectedKey={selectedTab}
      onSelectionChange={selectTab}
      tabs={shownTabs.map((tab) => ({ id: tab, label: t(tabLabelKeys[tab]), count: countByTab[tab] }))}
    >
      <Tabs.Panel id='live'>{
        renderList(liveStatus, false)
      }</Tabs.Panel>
      <Tabs.Panel id='deleted'>{
        renderList(deletedStatus, true)
      }</Tabs.Panel>
      {isAdmin && <Tabs.Panel id='feed'>
        <LearningFeed />
      </Tabs.Panel>}
    </PageTabs>
  </Page>
}
