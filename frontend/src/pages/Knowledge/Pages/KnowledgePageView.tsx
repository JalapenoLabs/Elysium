// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { PageAction } from '../../../layout/page/pageActions'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Navigate, useParams, useSearchParams } from 'react-router'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectOpenedPage, selectPageTree } from '../../../store/pagesSlice'

// User interface
import { buttonVariants, Chip, Link, Spinner, Tabs } from '@heroui/react'
import { LuHistory, LuPencil, LuSparkles } from 'react-icons/lu'
import { PageTabs } from '../../../components/PageTabs'
import { Page } from '../../../layout/page/Page'
import { ChildPageList } from './ChildPageList'
import { LearningFields } from './LearningFields'
import { PageArticle } from './PageArticle'
import { PageAttachments } from './PageAttachments'
import { PageByline } from './PageByline'
import { PageHistory } from './PageHistory'
import { PageLabels } from './PageLabels'
import { PageProjects } from './PageProjects'
import { PageSuggestions } from './PageSuggestions'
import { RestorePageButton } from './RestorePageButton'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { startCompanyDocReview } from '../../../api/routes/companyDocRoutes'
import { usePageLoader } from '../../../hooks/useServerData'
import { useTabParam } from '../../../hooks/useTabParam'
import { toast } from '../../../toast/toast'
import {
  getKnowledgePageEditUrl,
  getKnowledgePageUrl,
  getLearningEditUrl,
  getRunViewUrl,
  pageAddressById,
  PAGE_HISTORY_TAB,
  PAGE_SUGGESTIONS_TAB,
  TAB_PARAM,
  UrlTree,
} from '../../../urls'
import { readPassageParam } from './pagePassage'
import { findPageAncestors } from './pageTree'
import { usePassageRange } from './usePassageRange'
import { usePageAttachmentUpload } from './usePageAttachments'

const TABS = [ 'read', PAGE_SUGGESTIONS_TAB, PAGE_HISTORY_TAB ] as const
type Tab = typeof TABS[number]

const tabLabelKeys = {
  read: 'pages.tabs.read',
  [PAGE_SUGGESTIONS_TAB]: 'pages.tabs.suggestions',
  [PAGE_HISTORY_TAB]: 'pages.tabs.history',
} as const satisfies Record<Tab, ParseKeys<'knowledge'>>

type Props = {
  pageId: string
}

// One page: its path through the tree as the breadcrumb, its title, then the page itself. The tree
// already knows the title and path, so they show at once, and the text follows. A deleted page reads
// on with a chip, off the tree, as a link to it may outlive it.
function KnowledgePageReader(props: Props) {
  const { t } = useTranslation([ 'knowledge', 'common' ])
  const status = usePageLoader(props.pageId)
  const page = useAppSelector((state) => selectOpenedPage(state, props.pageId))
  const tree = useAppSelector(selectPageTree)
  const summary = tree.byId.get(props.pageId)
  const childPages = tree.childrenByParent.get(props.pageId) ?? []
  const [ selectedTab, selectTab ] = useTabParam(TABS)
  const [ params ] = useSearchParams()
  const passagePosition = readPassageParam(params)
  // Only Read shows the text, so History asks for nothing, and coming back opens at the passage.
  const markedRange = usePassageRange(page, selectedTab === 'read' ? passagePosition : null)
  const upload = usePageAttachmentUpload(props.pageId)
  const [ isStartingReview, setIsStartingReview ] = useState(false)

  // The company doc's review runs every few hours; Look for updates starts one now and says so,
  // linking to its run.
  async function startReview() {
    setIsStartingReview(true)
    try {
      const response = await startCompanyDocReview()
      const message = response.started
        ? t('pages.review.started')
        : t('pages.review.running')
      toast.success(message, {
        link: { label: t('pages.review.openRun'), href: getRunViewUrl(response.run.id) },
      })
    }
    catch (error) {
      console.debug('KnowledgePageReader could not start a company doc review', { error })
      toast.danger(t('common:errors.unexpected'), { description: getApiErrorMessage(error) ?? undefined })
    }
    finally {
      setIsStartingReview(false)
    }
  }

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
    // A learning's undo stays on its own page.
    if (page.category) {
      actions.push({
        id: 'learningHistory',
        label: t('pages.learningHistory'),
        icon: LuHistory,
        href: getLearningEditUrl(page.id),
      })
    }
    if (page.companyDoc) {
      actions.push({
        id: 'lookForUpdates',
        label: t('pages.lookForUpdates'),
        icon: LuSparkles,
        isPending: isStartingReview,
        onAction: () => void startReview(),
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
        // Both counts come with the page, so neither needs a load and its text never waits on one:
        // versions number from one, and the page counts what waits for review.
        count: tabCount(tab, page.currentVersion, page.pendingSuggestions),
      }))}
    >
      <Tabs.Panel id='read' className='mt-0 p-0'>
        <PageArticle
          key={passagePosition}
          markdown={page.body}
          markedRange={markedRange}
          byline={<PageByline page={page} />}
          labels={<PageLabels page={page} />}
          projects={<PageProjects page={page} />}
          learning={page.category && page.evidence
            ? <LearningFields category={page.category} evidence={page.evidence} />
            : undefined}
          attachments={<PageAttachments page={page} upload={upload} />}
          childPages={childPages.length
            ? <ChildPageList pages={childPages} />
            : undefined}
        />
      </Tabs.Panel>
      <Tabs.Panel id={PAGE_SUGGESTIONS_TAB} className='mt-0 p-0'>
        <PageSuggestions page={page} />
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

// What a tab counts: a page's versions, and the suggestions waiting on it, which shows no count
// while none wait.
function tabCount(tab: Tab, versions: number, pendingSuggestions: number) {
  if (tab === PAGE_HISTORY_TAB) {
    return versions
  }
  if (tab === PAGE_SUGGESTIONS_TAB && pendingSuggestions) {
    return pendingSuggestions
  }
  return null
}

// `/knowledge/pages/:pageId`. Keyed by the page, so moving between pages starts a fresh `Page` and
// nothing of one, such as an edit, carries to the next.
export function KnowledgePageView() {
  const { pageId = '' } = useParams()
  const [ params ] = useSearchParams()

  // A seeded page shown as a page of its own, such as the Meetings list, reads there. Its history
  // still reads here, at the address the tree never offers.
  const ownAddress = pageAddressById[pageId]
  if (ownAddress && !params.has(TAB_PARAM)) {
    return <Navigate to={ownAddress} replace />
  }
  return <KnowledgePageReader key={pageId} pageId={pageId} />
}
