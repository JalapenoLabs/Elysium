// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { PageAction } from '../../../layout/page/pageActions'

// Core
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectOpenedPage, selectPageTree } from '../../../store/pagesSlice'

// User interface
import { buttonVariants, Chip, Link, Spinner, Tabs } from '@heroui/react'
import { LuHistory, LuMessagesSquare, LuPencil } from 'react-icons/lu'
import { PageTabs } from '../../../components/PageTabs'
import { Page } from '../../../layout/page/Page'
import { ChildPageList } from './ChildPageList'
import { LearningFields } from './LearningFields'
import { PageArticle } from './PageArticle'
import { PageByline } from './PageByline'
import { PageHistory } from './PageHistory'
import { RestorePageButton } from './RestorePageButton'

// Misc
import { usePageLoader } from '../../../hooks/useServerData'
import { useTabParam } from '../../../hooks/useTabParam'
import {
  getKnowledgePageEditUrl,
  getKnowledgePageUrl,
  getLearningEditUrl,
  PAGE_HISTORY_TAB,
  UrlTree,
} from '../../../urls'
import { findPageAncestors } from './pageTree'

const TABS = [ 'read', PAGE_HISTORY_TAB ] as const
type Tab = typeof TABS[number]

const tabLabelKeys = {
  read: 'pages.tabs.read',
  [PAGE_HISTORY_TAB]: 'pages.tabs.history',
} as const satisfies Record<Tab, ParseKeys<'knowledge'>>

type Props = {
  pageId: string
}

// One page: its path through the tree as the breadcrumb, its title, then the page itself. The tree
// already knows the title and path, so they show at once, and the text follows. A deleted page reads
// on with a chip, off the tree, as a link to it may outlive it.
function KnowledgePageReader(props: Props) {
  const { t } = useTranslation('knowledge')
  const status = usePageLoader(props.pageId)
  const page = useAppSelector((state) => selectOpenedPage(state, props.pageId))
  const tree = useAppSelector(selectPageTree)
  const summary = tree.byId.get(props.pageId)
  const childPages = tree.childrenByParent.get(props.pageId) ?? []
  const [ selectedTab, selectTab ] = useTabParam(TABS)

  const parents = [
    { label: t('title'), href: UrlTree.knowledge },
    ...findPageAncestors(tree, props.pageId).map((ancestor) => ({
      label: ancestor.title,
      href: getKnowledgePageUrl(ancestor.id),
    })),
  ]

  const titleWhileLoading = status === 'failed'
    ? t('pages.unavailableTitle')
    : t('pages.loading')

  const actions: PageAction[] = []
  if (page && !page.deletedAt) {
    // A learning's undo and the company doc's proposals stay on their own pages.
    if (page.category) {
      actions.push({
        id: 'learningHistory',
        label: t('pages.learningHistory'),
        icon: LuHistory,
        href: getLearningEditUrl(page.id),
      })
    }
    else if (page.proposalOnly) {
      actions.push({
        id: 'companyDocProposals',
        label: t('pages.companyDocProposals'),
        icon: LuMessagesSquare,
        href: UrlTree.settingsCompanyDoc,
      })
    }
  }

  return <Page
    title={page?.title ?? summary?.title ?? titleWhileLoading}
    parents={parents}
    badges={page?.deletedAt && <Chip size='sm' color='danger' variant='soft'>{t('pages.deleted')}</Chip>}
    primaryAction={page?.deletedAt
      ? <RestorePageButton page={page} />
      : page && <Link href={getKnowledgePageEditUrl(page.id)} className={buttonVariants({ variant: 'outline' })}>
        <LuPencil className='size-4' aria-hidden />
        <span>{t('pages.edit')}</span>
      </Link>}
    actions={actions}
  >
    {page && <PageTabs
      label={t('pages.tabs.label')}
      selectedKey={selectedTab}
      onSelectionChange={selectTab}
      tabs={TABS.map((tab) => ({
        id: tab,
        label: t(tabLabelKeys[tab]),
        // Versions number from one, so the count needs no load and a page's text never waits on it.
        count: tab === PAGE_HISTORY_TAB
          ? page.currentVersion
          : null,
      }))}
    >
      <Tabs.Panel id='read' className='mt-0 p-0'>
        <PageArticle
          markdown={page.body}
          byline={<PageByline page={page} />}
          learning={page.category && page.evidence
            ? <LearningFields category={page.category} evidence={page.evidence} />
            : undefined}
          childPages={childPages.length
            ? <ChildPageList pages={childPages} />
            : undefined}
        />
      </Tabs.Panel>
      <Tabs.Panel id={PAGE_HISTORY_TAB} className='mt-0 p-0'>
        <PageHistory page={page} />
      </Tabs.Panel>
    </PageTabs>}
    {!page && status === 'loading' && <div className='grid place-items-center py-16'>
      <Spinner />
    </div>}
    {!page && status === 'failed' && <div className='py-10 text-center text-sm'>
      <p className='compact opacity-70'>{t('pages.unavailable')}</p>
      <Link href={UrlTree.knowledge} className='text-link'>{t('pages.backToKnowledge')}</Link>
    </div>}
  </Page>
}

// `/knowledge/pages/:pageId`. Keyed by the page, so moving between pages starts a fresh `Page` and
// nothing of one, such as an edit, carries to the next.
export function KnowledgePageView() {
  const { pageId = '' } = useParams()

  return <KnowledgePageReader key={pageId} pageId={pageId} />
}
