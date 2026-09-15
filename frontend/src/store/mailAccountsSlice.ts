// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { MailAccount } from '../api/routes/mailRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSlice } from '@reduxjs/toolkit'

const mailAccountsAdapter = createEntityAdapter<MailAccount>({
  sortComparer: (first, second) => first.address.localeCompare(second.address),
})

export const mailAccountsSlice = createSlice({
  name: 'mailAccounts',
  initialState: mailAccountsAdapter.getInitialState(),
  reducers: {
    mailAccountsLoaded: mailAccountsAdapter.setAll,
    mailAccountUpserted: mailAccountsAdapter.upsertOne,
    mailAccountDeleted(state, action: PayloadAction<string>) {
      mailAccountsAdapter.removeOne(state, action.payload)
    },
  },
})

export const { mailAccountsLoaded, mailAccountUpserted, mailAccountDeleted } = mailAccountsSlice.actions

export const {
  selectAll: selectAllMailAccounts,
} = mailAccountsAdapter.getSelectors((state: RootState) => state.mailAccounts)
