// Copyright © 2026 Jalapeno Labs

import type { IconType } from 'react-icons'
import type { LoadStatus } from '../../hooks/useServerData'
import type { PageTree as PageTreeShape } from './Pages/pageTree'
import type { PageTreeMemoryState } from './Pages/usePageTreeMemory'

// Core
import { useTranslation } from 'react-i18next'
import { useMatch } from 'react-router'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectAllKnowledgeAsks } from '../../store/knowledgeAsksSlice'
import { selectLiveLearnings } from '../../store/learningsSlice'

// User interface
import { buttonVariants, Link as HeroLink, Separator, Tooltip } from '@heroui/react'
import { Link } from 'react-aria-components'
import { LuLightbulb, LuPlus, LuSearch, LuSparkles } from 'react-icons/lu'
import { PageTree } from './Pages/PageTree'

// Misc
import { formatNumber } from '../../format/numbers'
import { useKnowledgeAsksLoader, useLearningsLoader } from '../../hooks/useServerData'
import { UrlTree } from '../../urls'

type Props = {
  tree: PageTreeShape
  treeStatus: LoadStatus
  memory: PageTreeMemoryState
  // Following anything in it: the phone's drawer closes.
  onNavigate?: () => void
}

type SectionLinkProps = {
  href: string
  label: string
  icon: IconType
  // Shows once what it counts has loaded; the search has no total of its own.
  count: number | null
  onNavigate?: () => void
}

function SectionLink(props: SectionLinkProps) {
  const isActive = Boolean(useMatch({ path: props.href, end: false }))
  const Icon = props.icon

  return <Link
    href={props.href}
    aria-current={isActive
      ? 'page'
      : undefined}
    onPress={props.onNavigate}
    className={[
      'flex items-center gap-2.5 rounded-md px-2.5 py-1.5 text-sm outline-none',
      'transition-colors duration-150 motion-reduce:transition-none',
      'data-focus-visible:ring-2 data-focus-visible:ring-focus data-focus-visible:ring-inset',
      isActive
        ? 'bg-accent/15 font-medium text-accent'
        : 'text-foreground/80 data-hovered:bg-default data-hovered:text-foreground',
    ].join(' ')}
  >
    <Icon className='size-4 shrink-0' aria-hidden />
    <span className='flex-1'>{props.label}</span>
    {props.count !== null && <span className='text-xs opacity-70'>{formatNumber(props.count)}</span>}
  </Link>
}

// What the knowledge base holds, beside everything in it: its search, asking it, and its
// learnings, then every page. One panel, so a person reading a page is a click from the search,
// and a person searching keeps their place in the tree.
export function KnowledgePanel(props: Props) {
  const { t } = useTranslation('knowledge')
  const learningsStatus = useLearningsLoader()
  const learnings = useAppSelector(selectLiveLearnings)
  const asksStatus = useKnowledgeAsksLoader()
  const asks = useAppSelector(selectAllKnowledgeAsks)
  const hasPages = props.tree.byId.size > 0

  return <div className='flex min-h-0 flex-1 flex-col gap-3'>
    <nav aria-label={t('sections.label')} className='flex flex-col gap-0.5'>
      <SectionLink
        href={UrlTree.knowledgeSearch}
        label={t('sections.search')}
        icon={LuSearch}
        count={null}
        onNavigate={props.onNavigate}
      />
      <SectionLink
        href={UrlTree.knowledgeAsk}
        label={t('sections.ask')}
        icon={LuSparkles}
        count={asksStatus === 'loaded'
          ? asks.length
          : null}
        onNavigate={props.onNavigate}
      />
      <SectionLink
        href={UrlTree.knowledgeLearnings}
        label={t('sections.learnings')}
        icon={LuLightbulb}
        count={learningsStatus === 'loaded'
          ? learnings.length
          : null}
        onNavigate={props.onNavigate}
      />
    </nav>
    <Separator />
    <div className='flex items-center justify-between pl-2.5'>
      <h2 className='text-xs font-medium opacity-70'>{t('pages.heading')}</h2>
      <Tooltip delay={300}>
        <HeroLink
          href={UrlTree.knowledgePageNew}
          aria-label={t('pages.new')}
          className={buttonVariants({ isIconOnly: true, size: 'sm', variant: 'ghost' })}
          onPress={props.onNavigate}
        >
          <LuPlus className='size-4' aria-hidden />
        </HeroLink>
        <Tooltip.Content>
          <span>{t('pages.new')}</span>
        </Tooltip.Content>
      </Tooltip>
    </div>
    {hasPages && <PageTree
      tree={props.tree}
      openIds={props.memory.openIds}
      onOpenIdsChange={props.memory.setOpenIds}
      onOpenBranches={props.memory.openBranches}
      onNavigate={props.onNavigate}
    />}
    {!hasPages && props.treeStatus === 'loaded' && <p className='px-2.5 text-sm opacity-70'>{t('pages.none')}</p>}
    {props.treeStatus === 'failed' && <p role='status' className='px-2.5 text-sm text-warning'>{
      t('pages.treeLoadFailed')
    }</p>}
  </div>
}
