// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { ConnectorAccount } from '../api/routes/connectorRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSlice } from '@reduxjs/toolkit'

const connectorAccountsAdapter = createEntityAdapter<ConnectorAccount>({
  sortComparer: (first, second) => first.createdAt.localeCompare(second.createdAt),
})

export const connectorAccountsSlice = createSlice({
  name: 'connectorAccounts',
  initialState: connectorAccountsAdapter.getInitialState(),
  reducers: {
    connectorAccountsLoaded: connectorAccountsAdapter.setAll,
    connectorAccountUpserted: connectorAccountsAdapter.upsertOne,
    connectorAccountDeleted(state, action: PayloadAction<string>) {
      connectorAccountsAdapter.removeOne(state, action.payload)
    },
  },
})

export const {
  connectorAccountsLoaded,
  connectorAccountUpserted,
  connectorAccountDeleted,
} = connectorAccountsSlice.actions

export const {
  selectAll: selectAllConnectorAccounts,
} = connectorAccountsAdapter.getSelectors((state: RootState) => state.connectorAccounts)
