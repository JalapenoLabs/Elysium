// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'

// Core
import { useTranslation } from 'react-i18next'
import { Outlet, useLocation } from 'react-router'

// Redux
import { selectIsCompanyMember } from '../../store/authSlice'
import { useAppSelector } from '../../store/hooks'
import { selectLiveLearnings } from '../../store/learningsSlice'

// User interface
import { buttonVariants, Link } from '@heroui/react'
import { LuPlus } from 'react-icons/lu'
import { EmptyNotice } from '../../components/EmptyNotice'
import { PageTabs } from '../../components/PageTabs'
import { Page } from '../../layout/page/Page'

// Misc
import { useLearningsLoader } from '../../hooks/useServerData'
import { UrlTree } from '../../urls'

const SECTIONS = [ 'search', 'learnings' ] as const
type KnowledgeSection = typeof SECTIONS[number]

const sectionUrls = {
  search: UrlTree.knowledge,
  learnings: UrlTree.knowledgeLearnings,
} as const satisfies Record<KnowledgeSection, string>

const sectionLabelKeys = {
  search: 'tabs.search',
  learnings: 'tabs.learnings',
} as const satisfies Record<KnowledgeSection, ParseKeys<'knowledge'>>

// The frame of the knowledge base's views: its search (the default) and its learnings, as tabs
// under one heading. A document and a learning stand on their own with breadcrumbs back here.
// A guest reaches no knowledge base (docs/knowledge.md, Who sees what), so they are told so
// rather than shown views the API refuses them.
export function KnowledgeLayout() {
  const { t } = useTranslation([ 'knowledge', 'learnings' ])
  const location = useLocation()
  const isCompanyMember = useAppSelector(selectIsCompanyMember)
  const learningsStatus = useLearningsLoader()
  const learnings = useAppSelector(selectLiveLearnings)

  if (!isCompanyMember) {
    return <Page title={t('title')}>
      <EmptyNotice>{t('membersOnly')}</EmptyNotice>
    </Page>
  }

  const selectedSection = SECTIONS.find((section) => sectionUrls[section] === location.pathname)
    ?? 'search'
  // A tab's count shows once what it counts has loaded; the search has no total of its own.
  const countBySection = {
    search: null,
    learnings: learningsStatus === 'loaded'
      ? learnings.length
      : null,
  } as const satisfies Record<KnowledgeSection, number | null>

  return <Page
    title={t('title')}
    primaryAction={selectedSection === 'learnings' && <Link
      href={UrlTree.knowledgeLearningNew}
      className={buttonVariants({ variant: 'outline' })}
    >
      <LuPlus className='size-4' aria-hidden />
      <span>{t('learnings:new')}</span>
    </Link>}
  >
    <PageTabs
      label={t('tabs.label')}
      selectedKey={selectedSection}
      tabs={SECTIONS.map((section) => ({
        id: section,
        label: t(sectionLabelKeys[section]),
        count: countBySection[section],
        href: sectionUrls[section],
      }))}
    />

    <Outlet />
  </Page>
}
