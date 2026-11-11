// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { KnowledgePage, KnowledgePageSummary } from '../api/routes/pageRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSelector, createSlice } from '@reduxjs/toolkit'

// Misc
import { buildPageTree, collectLabelsInUse } from '../pages/Knowledge/Pages/pageTree'

// In order among siblings, as the API lists them, so nesting them keeps that order.
const summariesAdapter = createEntityAdapter<KnowledgePageSummary>({
  sortComparer: (first, second) => first.position - second.position,
})

// The tree holds every live page without its text; a page's text is held only once someone opens
// it, so a big tree never costs its pages' bodies.
const initialState = {
  summaries: summariesAdapter.getInitialState(),
  openedById: {} as Record<string, KnowledgePage>,
}

export const pagesSlice = createSlice({
  name: 'pages',
  initialState,
  reducers: {
    pagesLoaded(state, action: PayloadAction<KnowledgePageSummary[]>) {
      summariesAdapter.setAll(state.summaries, action.payload)
    },
    // A page read whole, which is also all the tree holds of it, so a publish names itself in the
    // tree at once. A deleted page leaves the tree; the pages its deletion took follow with the
    // next load of the list, which `page.changed` brings.
    pageLoaded(state, action: PayloadAction<KnowledgePage>) {
      const page = action.payload
      state.openedById[page.id] = page
      if (page.deletedAt) {
        summariesAdapter.removeOne(state.summaries, page.id)
        return
      }
      summariesAdapter.setOne(state.summaries, {
        id: page.id,
        parentId: page.parentId,
        position: page.position,
        title: page.title,
        currentVersion: page.currentVersion,
        fixed: page.fixed,
        briefProjectId: page.briefProjectId,
        labels: page.labels,
        pendingSuggestions: page.pendingSuggestions,
        updatedAt: page.updatedAt,
        deletedAt: page.deletedAt,
      })
    },
  },
})

export const { pagesLoaded, pageLoaded } = pagesSlice.actions

const { selectAll: selectPageSummaries } = summariesAdapter.getSelectors(
  (state: RootState) => state.pages.summaries,
)

export const selectPageTree = createSelector([ selectPageSummaries ], buildPageTree)

// Every label on a live page, which suggestions and filters offer.
export const selectLabelsInUse = createSelector([ selectPageSummaries ], collectLabelsInUse)

export function selectOpenedPage(state: RootState, pageId: string): KnowledgePage | undefined {
  return state.pages.openedById[pageId]
}
