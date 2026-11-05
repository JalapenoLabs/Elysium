// Copyright © 2026 Jalapeno Labs

import type { DropTarget, Key } from 'react-aria-components'
import type { KnowledgePageSummary, PagePlacement } from '../../../api/routes/pageRoutes'
import type { PageTree as PageTreeShape } from './pageTree'

// Core
import { memo, useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useLocation } from 'react-router'

// User interface
import { Tooltip } from '@heroui/react'
import {
  Button,
  DropIndicator,
  Link,
  NavigationTree,
  NavigationTreeItem,
  NavigationTreeItemContent,
  TreeContext,
  useDragAndDrop,
} from 'react-aria-components'
import { LuChevronRight, LuGripVertical, LuLock } from 'react-icons/lu'
import { MovePageModal } from './MovePageModal'
import { PageRowActions } from './PageRowActions'

// Misc
import { PAGE_DRAG_TYPE } from '../../../constants'
import { getKnowledgePageUrl } from '../../../urls'
import { findDropPlacement } from './pageTree'
import { usePageActions } from './usePageActions'

type Props = {
  tree: PageTreeShape
  openIds: Set<string>
  onOpenIdsChange: (ids: Set<Key>) => void
  // Shows a page's new place when it lands under a closed branch.
  onOpenBranches: (ids: string[]) => void
  onNavigate?: () => void
}

type ItemProps = Pick<Props, 'tree' | 'onNavigate'> & {
  page: KnowledgePageSummary
  onMove: (page: KnowledgePageSummary) => void
  onDelete: (page: KnowledgePageSummary) => void
}

// Hovering fills a row neutrally and the current page is accent-colored, so the two never look
// alike (`~/.claude/docs/ux.md`, Navigation). A row a drag would drop on takes the accent outright,
// and the row being dragged fades, so both read whatever the row was showing.
const ROW_CLASS_NAME = [
  'group/page-row relative rounded-md text-sm text-foreground/80 outline-none',
  'transition-colors duration-150 motion-reduce:transition-none',
  'not-data-current:hover:bg-default not-data-current:hover:text-foreground',
  'data-current:bg-accent/15 data-current:font-medium data-current:text-accent',
  'data-focus-visible:ring-2 data-focus-visible:ring-focus data-focus-visible:ring-inset',
  'data-dragging:opacity-50',
  'data-drop-target:bg-accent/30 data-drop-target:ring-2 data-drop-target:ring-accent data-drop-target:ring-inset',
].join(' ')

// Shown over a row's end only when it is wanted, so a title keeps the panel's width the rest of the
// time. Fading rather than hiding keeps every control in the tab order. A touch screen has no hover,
// so its menu always shows.
const ROW_CONTROLS_CLASS_NAME = [
  'absolute inset-y-0 right-1 flex items-center gap-0.5 opacity-0',
  'transition-opacity duration-150 motion-reduce:transition-none',
  'group-hover/page-row:opacity-100 group-focus-within/page-row:opacity-100',
  'has-[[aria-expanded=true]]:opacity-100 pointer-coarse:opacity-100',
].join(' ')

// Every page is a node of the tree, open or not: React Aria learns a row can open from its nodes,
// for `aria-expanded` and the arrow keys, and draws only the rows that show. Memoized, so opening a
// branch costs React Aria's walk over what shows rather than a render of every page.
const PageTreeItem = memo(function PageTreeItem(props: ItemProps) {
  const { page, tree, onNavigate, onMove, onDelete } = props
  const { t } = useTranslation('knowledge')

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
        <Link onPress={onNavigate} className={[
          'min-w-0 flex-1 truncate py-1 outline-none',
          // Makes room for the controls while they show.
          'group-hover/page-row:pr-12 group-focus-within/page-row:pr-12 pointer-coarse:pr-8',
        ].join(' ')}>
          {/* Cut to the panel's width, so the whole title shows on hover. */}
          <span title={page.title}>{page.title}</span>
        </Link>
        <div className={ROW_CONTROLS_CLASS_NAME}>
          {page.fixed && <Tooltip delay={300}>
            <Tooltip.Trigger
              aria-label={t('pages.actions.fixed')}
              className='grid size-6 shrink-0 place-items-center rounded pointer-coarse:sr-only'
            >
              <LuLock aria-hidden className='size-3.5' />
            </Tooltip.Trigger>
            <Tooltip.Content>
              <span>{t('pages.actions.fixed')}</span>
            </Tooltip.Content>
          </Tooltip>}
          {/* Keyboards and screen readers start a drag here; a pointer drags the row itself. A
              fixed page's stays disabled and out of sight, since React Aria expects every row to
              have one. */}
          <Button
            slot='drag'
            isDisabled={page.fixed}
            aria-label={t('pages.actions.drag', { title: page.title })}
            className={[
              'grid size-6 shrink-0 place-items-center rounded outline-none',
              'focus-visible:ring-2 focus-visible:ring-focus pointer-coarse:sr-only',
              page.fixed
                ? 'hidden'
                : '',
            ].join(' ')}
          >
            <LuGripVertical aria-hidden className='size-3.5' />
          </Button>
          <PageRowActions page={page} onMove={onMove} onDelete={onDelete} onNavigate={onNavigate} />
        </div>
      </div>
    }</NavigationTreeItemContent>
    {tree.childrenByParent.get(page.id)?.map((child) => <PageTreeItem
      key={child.id}
      page={child}
      tree={tree}
      onNavigate={onNavigate}
      onMove={onMove}
      onDelete={onDelete}
    />)}
  </NavigationTreeItem>
})

function renderDragPreview(items: Record<string, string>[]) {
  return <div className='rounded-md bg-surface px-3 py-1.5 text-sm shadow-md'>{items[0]?.['text/plain']}</div>
}

// Where a drag would land: a line at that row's depth, there only while it is the target.
function renderDropIndicator(target: DropTarget) {
  return <DropIndicator target={target} className={[
    'relative z-10 h-0 outline-none',
    'data-drop-target:before:absolute data-drop-target:before:-top-px data-drop-target:before:right-1',
    'data-drop-target:before:left-[calc((min(var(--tree-item-level),8)-1)*0.875rem+0.25rem)]',
    'data-drop-target:before:h-0.5 data-drop-target:before:rounded-full data-drop-target:before:bg-accent',
  ].join(' ')} />
}

// Every page, each a real link, with the one being read highlighted and its branches opened by
// `KnowledgeFrame`. React Aria's `NavigationTree` gives it a tree's arrow keys and scrolling the
// current page into view, and its drag and drop reorders and moves pages by pointer and by keyboard,
// refusing a target under the page being dragged. Dragging can't happen everywhere, so each row's
// menu offers the same move with a picker.
export function PageTree(props: Props) {
  const { t } = useTranslation('knowledge')
  const { pathname } = useLocation()
  const [ movingPage, setMovingPage ] = useState<KnowledgePageSummary | null>(null)
  const actions = usePageActions(props.tree)

  async function moveAndShow(page: KnowledgePageSummary, placement: PagePlacement) {
    const isMoved = await actions.move(page, placement)
    if (isMoved && placement.parentId) {
      props.onOpenBranches([ placement.parentId ])
    }
    return isMoved
  }

  // Rows are memoized, so they keep one function, which reaches for the newest tree when pressed.
  const removeRef = useRef(actions.remove)
  useEffect(() => {
    removeRef.current = actions.remove
  })
  const onDelete = useCallback((page: KnowledgePageSummary) => removeRef.current(page), [])

  const { dragAndDropHooks } = useDragAndDrop<KnowledgePageSummary>({
    // React Aria's only switch for one row's drag disables the row whole, its link and arrow keys
    // too. So a fixed page drags as its title alone, which nothing here accepts, and its row has no
    // drag button for a keyboard to start one.
    getItems: (keys) => Array.from(keys, (key) => {
      const page = props.tree.byId.get(String(key))
      const item = { 'text/plain': page?.title ?? '' }
      if (!page || page.fixed) {
        return item
      }
      return { ...item, [PAGE_DRAG_TYPE]: page.id }
    }),
    getAllowedDropOperations: () => [ 'move' ],
    acceptedDragTypes: [ PAGE_DRAG_TYPE ],
    renderDragPreview,
    renderDropIndicator,
    onMove: (event) => {
      const pageId = String(Array.from(event.keys)[0])
      const page = props.tree.byId.get(pageId)
      const placement = findDropPlacement(props.tree, pageId, {
        key: String(event.target.key),
        dropPosition: event.target.dropPosition,
      })
      if (page && placement) {
        void moveAndShow(page, placement)
      }
    },
  })

  return <>
    {/* `NavigationTree` leaves drag and drop out of its own props; the `Tree` inside it reads them
        from its context. */}
    <TreeContext.Provider value={{ dragAndDropHooks }}>
      <NavigationTree
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
          onMove={setMovingPage}
          onDelete={onDelete}
        />)}
      </NavigationTree>
    </TreeContext.Provider>
    {movingPage && <MovePageModal
      key={movingPage.id}
      page={movingPage}
      tree={props.tree}
      onMove={(placement) => moveAndShow(movingPage, placement)}
      onClose={() => setMovingPage(null)}
    />}
  </>
}
