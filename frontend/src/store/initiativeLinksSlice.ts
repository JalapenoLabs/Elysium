// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { InitiativeLink } from '../api/routes/initiativeRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSelector, createSlice } from '@reduxjs/toolkit'

// Oldest first, as the API lists an initiative's containers.
const containersAdapter = createEntityAdapter<InitiativeLink>({
  sortComparer: (first, second) => first.createdAt.localeCompare(second.createdAt)
    || first.id.localeCompare(second.id),
})

export const initiativeLinksSlice = createSlice({
  name: 'initiativeLinks',
  initialState: containersAdapter.getInitialState(),
  reducers: {
    // Replaces one initiative's containers, so one unlinked while the page was away goes too.
    initiativeLinksLoaded(state, action: PayloadAction<{ initiativeId: string, links: InitiativeLink[] }>) {
      const staleIds: string[] = []
      for (const link of Object.values(state.entities)) {
        if (link.initiativeId === action.payload.initiativeId) {
          staleIds.push(link.id)
        }
      }
      containersAdapter.removeMany(state, staleIds)
      containersAdapter.setMany(state, action.payload.links)
    },
    initiativeLinkUpserted: containersAdapter.setOne,
    initiativeLinkDeleted(state, action: PayloadAction<string>) {
      containersAdapter.removeOne(state, action.payload)
    },
  },
})

export const {
  initiativeLinksLoaded,
  initiativeLinkUpserted,
  initiativeLinkDeleted,
} = initiativeLinksSlice.actions

const { selectAll: selectAllContainers } = containersAdapter.getSelectors(
  (state: RootState) => state.initiativeLinks,
)

export const selectInitiativeLinks = createSelector(
  [ selectAllContainers, (_state: RootState, initiativeId: string) => initiativeId ],
  (links, initiativeId) => links.filter((link) => link.initiativeId === initiativeId),
)
