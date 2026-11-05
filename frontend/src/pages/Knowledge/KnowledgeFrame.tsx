// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect, useEffectEvent, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { Outlet, useLocation, useMatch } from 'react-router'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectPageTree } from '../../store/pagesSlice'

// User interface
import { Button, Drawer, useOverlayState } from '@heroui/react'
import { LuPanelLeft } from 'react-icons/lu'
import { CollapseEdgeButton } from '../../components/CollapseEdgeButton'
import { KnowledgePanel } from './KnowledgePanel'

// Misc
import { usePagesLoader } from '../../hooks/useServerData'
import { UrlTree } from '../../urls'
import { KNOWLEDGE_CONTENT_CLASS_NAME } from './knowledgeContent'
import { findPageAncestors } from './Pages/pageTree'
import { usePageTreeMemory } from './Pages/usePageTreeMemory'

// The knowledge base for the company's members: its panel beside whatever page is open, each
// scrolling on its own, so the tree stays put while a long page scrolls. The panel needs an app
// sidebar's width to leave a page room to read, so below `lg` it moves into a drawer. From `lg` it
// collapses to its edge, and remembers that with the branches left open (`usePageTreeMemory`).
export function KnowledgeFrame() {
  const { t } = useTranslation('knowledge')
  const { pathname } = useLocation()
  const drawer = useOverlayState()
  const memory = usePageTreeMemory()
  const treeStatus = usePagesLoader()
  const tree = useAppSelector(selectPageTree)
  const currentPageId = useMatch(UrlTree.knowledgePage)?.params.pageId
  const scrollerRef = useRef<HTMLDivElement>(null)

  // The router restores nothing inside a scroller of the page's own, so each address starts at
  // its top, as a page in the padded `main` does.
  useEffect(() => {
    scrollerRef.current?.scrollTo({ top: 0 })
  }, [ pathname ])

  // A page opened from a link shows where it sits. Once per page, so a branch closed afterwards
  // stays closed.
  const openBranchesToCurrentPage = useEffectEvent(() => {
    if (!currentPageId) {
      return
    }
    memory.openBranches(findPageAncestors(tree, currentPageId).map((page) => page.id))
  })
  useEffect(() => {
    openBranchesToCurrentPage()
  }, [ currentPageId, treeStatus ])

  return <div className='flex h-full'>
    <aside className={[
      'group/knowledge-panel relative hidden shrink-0 flex-col border-r border-separator bg-sidebar lg:flex',
      'transition-[width] duration-200 ease-out motion-reduce:transition-none',
      memory.isCollapsed
        ? 'w-4'
        : 'w-72 px-2 py-3',
    ].join(' ')}>
      <CollapseEdgeButton
        isCollapsed={memory.isCollapsed}
        collapseLabel={t('pages.collapse')}
        expandLabel={t('pages.expand')}
        onToggle={() => memory.setIsCollapsed(!memory.isCollapsed)}
        className={[
          'top-4 group-hover/knowledge-panel:opacity-100',
          // Nothing else marks where the panel went, so its way back always shows.
          memory.isCollapsed
            ? 'opacity-100'
            : '',
        ].join(' ')}
      />
      {!memory.isCollapsed && <KnowledgePanel tree={tree} treeStatus={treeStatus} memory={memory} />}
    </aside>

    <div ref={scrollerRef} className={`min-w-0 flex-1 ${KNOWLEDGE_CONTENT_CLASS_NAME}`}>
      <div className='compact lg:hidden'>
        <Button size='sm' variant='outline' onPress={drawer.open}>
          <LuPanelLeft className='size-4' aria-hidden />
          <span>{t('pages.browse')}</span>
        </Button>
      </div>
      <Outlet />
    </div>

    {/* Controlled, so the root trigger wrapper is skipped, as in MobileNavigation. */}
    <Drawer.Backdrop isOpen={drawer.isOpen} onOpenChange={drawer.setOpen}>
      <Drawer.Content placement='left'>
        <Drawer.Dialog className='flex flex-col gap-4 bg-sidebar'>
          <Drawer.Header>
            <Drawer.Heading className='text-lg'>{t('title')}</Drawer.Heading>
          </Drawer.Header>
          <Drawer.Body className='flex min-h-0 flex-1 flex-col'>
            <KnowledgePanel tree={tree} treeStatus={treeStatus} memory={memory} onNavigate={drawer.close} />
          </Drawer.Body>
          <Drawer.Footer className='flex-col items-stretch'>
            <Button fullWidth variant='secondary' onPress={drawer.close}>
              <span>{t('pages.closeBrowse')}</span>
            </Button>
          </Drawer.Footer>
        </Drawer.Dialog>
      </Drawer.Content>
    </Drawer.Backdrop>
  </div>
}
