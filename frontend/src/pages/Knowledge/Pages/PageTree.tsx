// Copyright © 2026 Jalapeno Labs

import type { KnowledgePageSummary } from '../../../api/routes/pageRoutes'
import type { Key } from 'react-aria-components'
import type { PageTree as PageTreeShape } from './pageTree'

// Core
import { memo } from 'react'
import { useTranslation } from 'react-i18next'
import { useLocation } from 'react-router'

// User interface
import {
  Button,
  Link,
  NavigationTree,
  NavigationTreeItem,
  NavigationTreeItemContent,
} from 'react-aria-components'
import { LuChevronRight } from 'react-icons/lu'

// Misc
import { getKnowledgePageUrl } from '../../../urls'

type Props = {
  tree: PageTreeShape
  openIds: Set<string>
  onOpenIdsChange: (ids: Set<Key>) => void
  onNavigate?: () => void
}

type ItemProps = Pick<Props, 'tree' | 'onNavigate'> & {
  page: KnowledgePageSummary
}

// Hovering fills a row neutrally and the current page is accent-colored, so the two never look
// alike (`~/.claude/docs/ux.md`, Navigation).
const ROW_CLASS_NAME = [
  'rounded-md text-sm text-foreground/80 outline-none',
  'transition-colors duration-150 motion-reduce:transition-none',
  'not-data-current:hover:bg-default not-data-current:hover:text-foreground',
  'data-current:bg-accent/15 data-current:font-medium data-current:text-accent',
  'data-focus-visible:ring-2 data-focus-visible:ring-focus data-focus-visible:ring-inset',
].join(' ')

// Every page is a node of the tree, open or not: React Aria learns a row can open from its nodes,
// for `aria-expanded` and the arrow keys, and draws only the rows that show. Memoized, so opening a
// branch costs React Aria's walk over what shows rather than a render of every page.
const PageTreeItem = memo(function PageTreeItem(props: ItemProps) {
  const { page, tree, onNavigate } = props

  return <NavigationTreeItem
    id={page.id}
    href={getKnowledgePageUrl(page.id)}
    textValue={page.title}
    className={ROW_CLASS_NAME}
  >
    <NavigationTreeItemContent>{
      (row) => <div className={[
        'flex min-w-0 items-center gap-0.5 py-0.5 pr-2',
        // React Aria names the row's depth; past eight levels a row stops indenting, so a deep
        // page's title keeps room in the panel.
        'pl-[calc((min(var(--tree-item-level),8)-1)*0.875rem)]',
      ].join(' ')}>
        {row.hasChildItems
          ? <Button
            slot='chevron'
            className={[
              'grid size-6 shrink-0 cursor-default place-items-center rounded outline-none',
              'hover:bg-foreground/10 focus-visible:ring-2 focus-visible:ring-focus',
            ].join(' ')}
          >
            <LuChevronRight
              aria-hidden
              className={[
                'size-3.5 transition-transform duration-150 motion-reduce:transition-none',
                row.isExpanded
                  ? 'rotate-90'
                  : 'rotate-0',
              ].join(' ')}
            />
          </Button>
          : <span aria-hidden className='size-6 shrink-0' />}
        {/* The row's real link: `NavigationTreeItem` hands it the address and `aria-current`. */}
        <Link onPress={onNavigate} className='min-w-0 flex-1 truncate py-1 outline-none'>
          {/* Cut to the panel's width, so the whole title shows on hover. */}
          <span title={page.title}>{page.title}</span>
        </Link>
      </div>
    }</NavigationTreeItemContent>
    {tree.childrenByParent.get(page.id)?.map((child) => <PageTreeItem
      key={child.id}
      page={child}
      tree={tree}
      onNavigate={onNavigate}
    />)}
  </NavigationTreeItem>
})

// Every page, each a real link, with the one being read highlighted and its branches opened by
// `KnowledgeFrame`. React Aria's `NavigationTree` gives it a tree's arrow keys, tabbing out of it
// in one press, and scrolling the current page into view.
export function PageTree(props: Props) {
  const { t } = useTranslation('knowledge')
  const { pathname } = useLocation()

  return <NavigationTree
    aria-label={t('pages.treeLabel')}
    selectedRoute={pathname}
    expandedKeys={props.openIds}
    onExpandedChange={props.onOpenIdsChange}
    className='min-h-0 flex-1 overflow-y-auto outline-none'
  >
    {props.tree.childrenByParent.get(null)?.map((page) => <PageTreeItem
      key={page.id}
      page={page}
      tree={props.tree}
      onNavigate={props.onNavigate}
    />)}
  </NavigationTree>
}
