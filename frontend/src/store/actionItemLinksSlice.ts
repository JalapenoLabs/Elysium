// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { ActionItemLink } from '../api/routes/actionItemRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSelector, createSlice } from '@reduxjs/toolkit'

// The primary first, then oldest first, as the API lists an item's links. The id breaks ties
// between links made in the same instant.
const linksAdapter = createEntityAdapter<ActionItemLink>({
  sortComparer: (first, second) => Number(second.isPrimary) - Number(first.isPrimary)
    || first.createdAt.localeCompare(second.createdAt)
    || first.id.localeCompare(second.id),
})

export const actionItemLinksSlice = createSlice({
  name: 'actionItemLinks',
  initialState: linksAdapter.getInitialState(),
  reducers: {
    // Replaces one item's links, so a link removed while the page was away goes too.
    actionItemLinksLoaded(state, action: PayloadAction<{ actionItemId: string, links: ActionItemLink[] }>) {
      const staleIds: string[] = []
      for (const link of Object.values(state.entities)) {
        if (link.actionItemId === action.payload.actionItemId) {
          staleIds.push(link.id)
        }
      }
      linksAdapter.removeMany(state, staleIds)
      linksAdapter.setMany(state, action.payload.links)
    },
    actionItemLinkUpserted: linksAdapter.setOne,
    actionItemLinkDeleted(state, action: PayloadAction<string>) {
      linksAdapter.removeOne(state, action.payload)
    },
  },
})

export const {
  actionItemLinksLoaded,
  actionItemLinkUpserted,
  actionItemLinkDeleted,
} = actionItemLinksSlice.actions

const { selectAll: selectAllLinks } = linksAdapter.getSelectors(
  (state: RootState) => state.actionItemLinks,
)

export const selectActionItemLinks = createSelector(
  [ selectAllLinks, (_state: RootState, actionItemId: string) => actionItemId ],
  (links, actionItemId) => links.filter((link) => link.actionItemId === actionItemId),
)
