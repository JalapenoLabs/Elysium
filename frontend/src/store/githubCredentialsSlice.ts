// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { GithubCredential } from '../api/routes/githubRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSelector, createSlice } from '@reduxjs/toolkit'

const githubCredentialsAdapter = createEntityAdapter<GithubCredential>({
  sortComparer: (first, second) => first.name.localeCompare(second.name),
})

export const githubCredentialsSlice = createSlice({
  name: 'githubCredentials',
  initialState: githubCredentialsAdapter.getInitialState(),
  reducers: {
    githubCredentialsLoaded: githubCredentialsAdapter.setAll,
    githubCredentialUpserted: githubCredentialsAdapter.upsertOne,
    githubCredentialDeleted(state, action: PayloadAction<string>) {
      githubCredentialsAdapter.removeOne(state, action.payload)
    },
  },
})

export const {
  githubCredentialsLoaded,
  githubCredentialUpserted,
  githubCredentialDeleted,
} = githubCredentialsSlice.actions

export const {
  selectAll: selectAllGithubCredentials,
  selectById: selectGithubCredentialById,
} = githubCredentialsAdapter.getSelectors((state: RootState) => state.githubCredentials)

// The workspace default, which sessions start with unless their project or the session
// chooses otherwise; null when none is set.
export const selectDefaultGithubCredential = createSelector(
  selectAllGithubCredentials,
  (credentials) => credentials.find((credential) => credential.isDefault) ?? null,
)
