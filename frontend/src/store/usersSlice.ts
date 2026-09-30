// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { User } from '../api/routes/userRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSelector, createSlice } from '@reduxjs/toolkit'

const usersAdapter = createEntityAdapter<User>({
  sortComparer: (first, second) => first.name.localeCompare(second.name),
})

export const usersSlice = createSlice({
  name: 'users',
  initialState: usersAdapter.getInitialState(),
  reducers: {
    usersLoaded: usersAdapter.setAll,
    userUpserted: usersAdapter.upsertOne,
    userDeleted(state, action: PayloadAction<string>) {
      usersAdapter.removeOne(state, action.payload)
    },
  },
})

export const { usersLoaded, userUpserted, userDeleted } = usersSlice.actions

export const {
  selectAll: selectAllUsers,
  selectById: selectUserById,
  selectEntities: selectUserEntities,
} = usersAdapter.getSelectors((state: RootState) => state.users)

// Names only, for views that say who wrote or did something. Pair with shallowEqual so
// those views do not rebuild when no name changed.
export const selectUserNamesById = createSelector(
  [ selectUserEntities ],
  (entities) => {
    const namesById: Record<string, string> = {}
    for (const user of Object.values(entities)) {
      namesById[user.id] = user.name
    }
    return namesById
  },
)

// The people an admin manages, split in one pass: sign-ups waiting for approval first,
// then everyone approved. Machines are not managed.
export const selectPeopleByStanding = createSelector(
  [ selectAllUsers ],
  (users) => {
    const pending: User[] = []
    const approved: User[] = []
    for (const user of users) {
      if (user.kind !== 'person') {
        continue
      }
      if (user.status === 'pending') {
        pending.push(user)
        continue
      }
      approved.push(user)
    }
    return { pending, approved }
  },
)
