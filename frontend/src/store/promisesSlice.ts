// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { PersonPromise } from '../api/routes/promiseRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSelector, createSlice } from '@reduxjs/toolkit'

const promisesAdapter = createEntityAdapter<PersonPromise>()

type FollowUpLoaded = {
  itemId: string
  // The promise the item follows up, or none.
  promises: PersonPromise[]
}

// The promises follow-up items' pages have loaded. A project's own promises arrive with its
// Project Manager (`projectManagersSlice`), whose page lists them.
export const promisesSlice = createSlice({
  name: 'promises',
  initialState: promisesAdapter.getInitialState(),
  reducers: {
    // An item's load replaces what is held for it, so a promise it no longer follows up does not
    // linger.
    followUpPromisesLoaded: (state, action: PayloadAction<FollowUpLoaded>) => {
      const stale: string[] = []
      for (const held of Object.values(state.entities)) {
        if (held.followUpItemId === action.payload.itemId) {
          stale.push(held.id)
        }
      }
      promisesAdapter.removeMany(state, stale)
      promisesAdapter.upsertMany(state, action.payload.promises)
    },
  },
})

export const { followUpPromisesLoaded } = promisesSlice.actions

const { selectAll } = promisesAdapter.getSelectors((state: RootState) => state.promises)

// The promise an item follows up, once its page loaded it; `null` for an item that follows up
// none, or until it loads.
export const selectPromiseByFollowUpItemId = createSelector(
  [ selectAll, (_state: RootState, itemId: string) => itemId ],
  (promises, itemId) => promises.find((made) => made.followUpItemId === itemId) ?? null,
)
