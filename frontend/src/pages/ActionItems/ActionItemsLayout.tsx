// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { ActionItemsSection } from './actionItemsSectionCounts'

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { Outlet, useLocation } from 'react-router'

// Redux
import { selectLiveActionItems } from '../../store/actionItemsSlice'
import { selectChangesets } from '../../store/changesetsSlice'
import { useAppSelector } from '../../store/hooks'
import { selectLiveInitiatives } from '../../store/initiativesSlice'

// User interface
import { buttonVariants, Link } from '@heroui/react'
import { LuPlus } from 'react-icons/lu'
import { PageTabs } from '../../components/PageTabs'
import { Page } from '../../layout/page/Page'

// Misc
import { useActionItemsLoader, useChangesetsLoader, useInitiativesLoader } from '../../hooks/useServerData'
import { useNow } from '../../hooks/useNow'
import { UrlTree } from '../../urls'
import { ACTION_ITEMS_SECTIONS, countActionItemsSections } from './actionItemsSectionCounts'

const sectionUrls = {
  next: UrlTree.actionItems,
  inbox: UrlTree.actionItemsInbox,
  all: UrlTree.actionItemsAll,
  initiatives: UrlTree.initiatives,
  changesets: UrlTree.changesets,
} as const satisfies Record<ActionItemsSection, string>

const sectionLabelKeys = {
  next: 'nav.next',
  inbox: 'nav.inbox',
  all: 'nav.all',
  initiatives: 'nav.initiatives',
  changesets: 'nav.changesets',
} as const satisfies Record<ActionItemsSection, ParseKeys<'actionItems'>>

// The two tabs whose count is what waits on the person, accented while anything does.
const waitingSections: readonly ActionItemsSection[] = [ 'inbox', 'changesets' ]

// The frame of the Action items area's five views: Next (the default), the inbox, every
// item, initiatives, and the changesets, as tabs under one heading, each with the total it
// lists when it opens. Item and initiative pages, and the create pages, stand on their own
// with breadcrumbs back here.
export function ActionItemsLayout() {
  const { t } = useTranslation([ 'actionItems', 'initiatives' ])
  const location = useLocation()
  const now = useNow()
  const itemsStatus = useActionItemsLoader()
  const initiativesStatus = useInitiativesLoader()
  const changesetsStatus = useChangesetsLoader()
  const items = useAppSelector(selectLiveActionItems)
  const initiatives = useAppSelector(selectLiveInitiatives)
  const changesets = useAppSelector(selectChangesets)
  const counts = useMemo(
    () => countActionItemsSections(items, initiatives, changesets, now),
    [ items, initiatives, changesets, now ],
  )
  // A tab's count shows once what it counts has loaded, so it never reads as zero meanwhile.
  const isLoadedBySection = {
    next: itemsStatus === 'loaded',
    inbox: itemsStatus === 'loaded',
    all: itemsStatus === 'loaded',
    initiatives: initiativesStatus === 'loaded',
    changesets: changesetsStatus === 'loaded',
  } as const satisfies Record<ActionItemsSection, boolean>

  const selectedSection = ACTION_ITEMS_SECTIONS.find((section) => sectionUrls[section] === location.pathname)
    ?? 'next'
  const isInitiatives = selectedSection === 'initiatives'

  return <Page
    title={t('title')}
    primaryAction={<Link
      href={isInitiatives
        ? UrlTree.initiativesNew
        : UrlTree.actionItemsNew}
      className={buttonVariants({ variant: 'outline' })}
    >
      <LuPlus className='size-4' aria-hidden />
      <span>{isInitiatives
        ? t('initiatives:newInitiative')
        : t('newItem')}</span>
    </Link>}
  >

    <PageTabs
      label={t('nav.label')}
      selectedKey={selectedSection}
      tabs={ACTION_ITEMS_SECTIONS.map((section) => ({
        id: section,
        label: t(sectionLabelKeys[section]),
        count: isLoadedBySection[section]
          ? counts[section]
          : null,
        isWaiting: waitingSections.includes(section),
        href: sectionUrls[section],
      }))}
    />

    <Outlet />
  </Page>
}
