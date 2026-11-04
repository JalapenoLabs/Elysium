// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { ParseKeys } from 'i18next'

// Core
import { useSearchParams } from 'react-router'
import { useTranslation } from 'react-i18next'

// Redux
import { selectIsAdmin, selectIsCompanyMember } from '../../../store/authSlice'
import { useAppSelector } from '../../../store/hooks'
import { selectDeletedLearnings, selectLiveLearnings } from '../../../store/learningsSlice'

// User interface
import { buttonVariants, Chip, Link, Spinner, Tabs } from '@heroui/react'
import { LuPlus } from 'react-icons/lu'
import { EmptyNotice } from '../../../components/EmptyNotice'
import { OptionSelect } from '../../../components/OptionSelect'
import { SettingsPage } from '../SettingsPage'
import { LearningFeed } from './LearningFeed'
import { LearningTable } from './LearningTable'

// Misc
import { useLearningCategories } from '../../../hooks/useLearningCategories'
import { useDeletedLearningsLoader, useLearningsLoader } from '../../../hooks/useServerData'
import { UrlTree } from '../../../urls'

const TABS = [ 'live', 'deleted', 'feed' ] as const
type Tab = typeof TABS[number]

const tabLabelKeys = {
  live: 'tabs.live',
  deleted: 'tabs.deleted',
  feed: 'tabs.feed',
} as const satisfies Record<Tab, ParseKeys<'learnings'>>

// The open tab and the category filter live in the address, so coming back from a learning
// returns to them.
const TAB_PARAM = 'tab'
const CATEGORY_PARAM = 'category'

// The filter's choice for every category, as the select's key; the address then names none.
const ANY_CATEGORY = 'any'

// `/settings/learnings`: every learning, the deleted ones, and, for admins, the feed of every
// change, each on its own tab. Both lists filter by category, built-in or the workspace's own.
// A guest reaches no knowledge base, so they are told so rather than shown lists the API refuses
// them. The Knowledge page folds these in once it is built.
export function LearningsPage() {
  const { t } = useTranslation([ 'learnings', 'common' ])
  const isAdmin = useAppSelector(selectIsAdmin)
  const isCompanyMember = useAppSelector(selectIsCompanyMember)
  const [ params, setParams ] = useSearchParams()
  const requestedTab = TABS.find((tab) => tab === params.get(TAB_PARAM)) ?? 'live'
  // Someone who is not an admin keeps the tabs they can see.
  const selectedTab = requestedTab === 'feed' && !isAdmin
    ? 'live'
    : requestedTab

  const liveStatus = useLearningsLoader()
  const deletedStatus = useDeletedLearningsLoader(isCompanyMember)
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

  function selectTab(key: Key) {
    const tab = TABS.find((candidate) => candidate === key)
    if (!tab) {
      console.debug('LearningsPage ignored an unknown tab', { key })
      return
    }
    setParams((current) => {
      const next = new URLSearchParams(current)
      next.set(TAB_PARAM, tab)
      return next
    }, { replace: true })
  }

  if (!isCompanyMember) {
    return <SettingsPage title={t('title')}>
      <EmptyNotice>{t('membersOnly')}</EmptyNotice>
    </SettingsPage>
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
  const shownTabs = TABS.filter((tab) => tab !== 'feed' || isAdmin)

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

  return <SettingsPage
    title={t('title')}
    primaryAction={<Link
      href={UrlTree.settingsLearningNew}
      className={buttonVariants({ variant: 'outline', className: 'gap-2' })}
    >
      <LuPlus className='size-4' aria-hidden />
      <span>{t('new')}</span>
    </Link>}
  >
    <Tabs variant='secondary' selectedKey={selectedTab} onSelectionChange={selectTab}>
      <Tabs.ListContainer className='relaxed'>
        <Tabs.List aria-label={t('tabs.label')}>{
          shownTabs.map((tab) => <Tabs.Tab key={tab} id={tab}>
            <span>{t(tabLabelKeys[tab])}</span>
            {countByTab[tab] !== null && <Chip size='sm' variant='soft' className='ml-2'>{
              countByTab[tab]
            }</Chip>}
            <Tabs.Indicator />
          </Tabs.Tab>)
        }</Tabs.List>
      </Tabs.ListContainer>
      <Tabs.Panel id='live'>{
        renderList(liveStatus, false)
      }</Tabs.Panel>
      <Tabs.Panel id='deleted'>{
        renderList(deletedStatus, true)
      }</Tabs.Panel>
      {isAdmin && <Tabs.Panel id='feed'>
        <LearningFeed />
      </Tabs.Panel>}
    </Tabs>
  </SettingsPage>
}
