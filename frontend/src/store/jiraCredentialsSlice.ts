// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { JiraCredential } from '../api/routes/jiraRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSlice } from '@reduxjs/toolkit'

const jiraCredentialsAdapter = createEntityAdapter<JiraCredential>({
  sortComparer: (first, second) => first.name.localeCompare(second.name),
})

export const jiraCredentialsSlice = createSlice({
  name: 'jiraCredentials',
  initialState: jiraCredentialsAdapter.getInitialState(),
  reducers: {
    jiraCredentialsLoaded: jiraCredentialsAdapter.setAll,
    jiraCredentialUpserted: jiraCredentialsAdapter.upsertOne,
    jiraCredentialDeleted(state, action: PayloadAction<string>) {
      jiraCredentialsAdapter.removeOne(state, action.payload)
    },
  },
})

export const {
  jiraCredentialsLoaded,
  jiraCredentialUpserted,
  jiraCredentialDeleted,
} = jiraCredentialsSlice.actions

export const {
  selectAll: selectAllJiraCredentials,
  selectById: selectJiraCredentialById,
} = jiraCredentialsAdapter.getSelectors((state: RootState) => state.jiraCredentials)
