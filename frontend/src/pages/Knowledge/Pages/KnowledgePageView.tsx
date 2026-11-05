// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectOpenedPage, selectPageTree } from '../../../store/pagesSlice'

// User interface
import { buttonVariants, Chip, Link, Spinner } from '@heroui/react'
import { LuPencil } from 'react-icons/lu'
import { Page } from '../../../layout/page/Page'
import { ChildPageList } from './ChildPageList'
import { LearningFields } from './LearningFields'
import { PageArticle } from './PageArticle'
import { PageByline } from './PageByline'

// Misc
import { usePageLoader } from '../../../hooks/useServerData'
import { getKnowledgePageUrl, getLearningEditUrl, UrlTree } from '../../../urls'
import { findPageAncestors } from './pageTree'

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

  // A learning is corrected, and the company doc edited and reviewed, on their own pages, where
  // their rules and history live.
  let editHref: string | null = null
  if (page?.category) {
    editHref = getLearningEditUrl(page.id)
  }
  else if (page?.proposalOnly) {
    editHref = UrlTree.settingsCompanyDoc
  }

  return <Page
    title={page?.title ?? summary?.title ?? titleWhileLoading}
    parents={parents}
    badges={page?.deletedAt && <Chip size='sm' color='danger' variant='soft'>{t('pages.deleted')}</Chip>}
    primaryAction={editHref && <Link href={editHref} className={buttonVariants({ variant: 'outline' })}>
      <LuPencil className='size-4' aria-hidden />
      <span>{t('pages.edit')}</span>
    </Link>}
  >
    {page && <PageArticle
      markdown={page.body}
      byline={<PageByline page={page} />}
      learning={page.category && page.evidence
        ? <LearningFields category={page.category} evidence={page.evidence} />
        : undefined}
      childPages={childPages.length
        ? <ChildPageList pages={childPages} />
        : undefined}
    />}
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
