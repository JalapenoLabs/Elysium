// Copyright © 2026 Jalapeno Labs

import type { KnowledgePageSummary, PagePlacement } from '../../../api/routes/pageRoutes'
import type { PageTree } from './pageTree'

// Core
import { useTranslation } from 'react-i18next'
import { useLocation, useMatch, useNavigate } from 'react-router'
import { mutate } from 'swr'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { pageLoaded } from '../../../store/pagesSlice'

// Misc
import { getApiErrorCode } from '../../../api/errors'
import { deletePage, movePage, pageKey, PAGES_KEY, restorePage } from '../../../api/routes/pageRoutes'
import { useConfirm } from '../../../hooks/useConfirm'
import { LEAVE_WITHOUT_ASKING } from '../../../layout/page/pageSave'
import { toast } from '../../../toast/toast'
import { getKnowledgePageUrl, UrlTree } from '../../../urls'
import { countPagesUnder, findPagePlacement, isPageUnder } from './pageTree'

const DELETE_CHILDREN_SWITCH = 'delete-children'

// Refusals a stale tree can run into, in the app's own words.
const REFUSAL_KEYS = {
  page_fixed: 'pages.refusals.fixed',
  page_under_itself: 'pages.refusals.underItself',
  page_parent_gone: 'pages.refusals.parentGone',
  page_placement_gone: 'pages.refusals.placementGone',
  page_deleted: 'pages.refusals.deleted',
} as const

const refusalKeys: Record<string, typeof REFUSAL_KEYS[keyof typeof REFUSAL_KEYS] | undefined> = REFUSAL_KEYS

// What can be done to a page from the tree. Each resolves to whether it happened.
export function usePageActions(tree: PageTree) {
  const { t } = useTranslation([ 'knowledge', 'common' ])
  const dispatch = useAppDispatch()
  const navigate = useNavigate()
  const { pathname } = useLocation()
  const confirm = useConfirm()
  const currentPageId = useMatch(UrlTree.knowledgePage)?.params.pageId

  // A refusal usually means this tree is behind someone else's change, so it loads again too.
  function reportFailure(error: unknown, context: string, page: KnowledgePageSummary) {
    console.debug(`usePageActions failed to ${context}`, { error, pageId: page.id })
    const refusalKey = refusalKeys[getApiErrorCode(error) ?? '']
    toast.danger(refusalKey
      ? t(refusalKey)
      : t('common:errors.unexpected'))
    void mutate(PAGES_KEY)
  }

  // Moves a page and answers at once for it; siblings a move renumbered follow with `page.changed`,
  // which is also how someone else's move reaches this tree.
  async function moveTo(page: KnowledgePageSummary, placement: PagePlacement) {
    try {
      const response = await movePage(page.id, placement)
      dispatch(pageLoaded(response.page))
      return true
    }
    catch (error) {
      reportFailure(error, 'move', page)
      return false
    }
  }

  async function move(page: KnowledgePageSummary, placement: PagePlacement) {
    const previous = findPagePlacement(tree, page)
    const isMoved = await moveTo(page, placement)
    if (isMoved) {
      toast.success(t('pages.move.moved', { title: page.title }), {
        action: {
          label: t('common:actions.undo'),
          onPress: () => void moveTo(page, previous),
        },
      })
    }
    return isMoved
  }

  // Whether the page being read leaves the tree with this deletion: it is the page, or, when its
  // children go too, anything under it.
  function isReadingWhatGoes(page: KnowledgePageSummary, keepChildren: boolean) {
    if (!currentPageId) {
      return false
    }
    if (currentPageId === page.id) {
      return true
    }
    return !keepChildren && isPageUnder(tree, currentPageId, page.id)
  }

  // Deleting is soft, so it happens at once, with Undo in its toast. Reading what went, the person
  // goes to the page's parent, and Undo brings them back to where they were.
  async function deleteNow(page: KnowledgePageSummary, keepChildren: boolean) {
    try {
      await deletePage(page.id, { keepChildren })
    }
    catch (error) {
      reportFailure(error, 'delete', page)
      return false
    }

    const isLeaving = isReadingWhatGoes(page, keepChildren)
    const leftFrom = pathname
    if (isLeaving) {
      const leaveTo = page.parentId
        ? getKnowledgePageUrl(page.parentId)
        : UrlTree.knowledge
      void navigate(leaveTo, { state: LEAVE_WITHOUT_ASKING })
    }

    // It answers nothing, and what it changed is more than one page, so the tree loads again.
    await mutate(PAGES_KEY)
    void mutate(pageKey(page.id))
    toast.success(t('pages.delete.deleted', { title: page.title }), {
      action: {
        label: t('common:actions.undo'),
        onPress: () => void restore(page, isLeaving
          ? leftFrom
          : null),
      },
    })
    return true
  }

  // Brings a deleted page back, with what its deletion took or moved out of it, and then the person
  // to `returnTo`, for an Undo that took them away.
  async function restore(page: KnowledgePageSummary, returnTo: string | null = null) {
    try {
      const response = await restorePage(page.id)
      dispatch(pageLoaded(response.page))
    }
    catch (error) {
      reportFailure(error, 'restore', page)
      return false
    }

    await mutate(PAGES_KEY)
    toast.success(t('pages.delete.restored', { title: page.title }))
    if (returnTo) {
      void navigate(returnTo)
    }
    return true
  }

  // A page with nothing under it goes at once. One with pages under it asks what happens to them:
  // left alone, they stay, moved up into the page's place.
  function remove(page: KnowledgePageSummary) {
    const childCount = tree.childrenByParent.get(page.id)?.length ?? 0
    if (!childCount) {
      void deleteNow(page, false)
      return
    }

    const parent = tree.byId.get(page.parentId ?? '')
    confirm({
      title: t('pages.delete.title', { title: page.title }),
      message: parent
        ? t('pages.delete.movesUp', { count: childCount, parent: parent.title })
        : t('pages.delete.movesToTop', { count: childCount }),
      tone: 'danger',
      confirmText: t('common:actions.delete'),
      cancelText: t('pages.move.cancel'),
      switches: [{
        id: DELETE_CHILDREN_SWITCH,
        label: t('pages.delete.alsoDelete', { count: countPagesUnder(tree, page.id) }),
      }],
      onConfirm: async (switchedOn) => {
        const isDeleted = await deleteNow(page, !switchedOn.has(DELETE_CHILDREN_SWITCH))
        if (!isDeleted) {
          // Keeps the dialog open to try again.
          throw new Error('The page was not deleted')
        }
      },
    })
  }

  return { move, remove, restore } as const
}
