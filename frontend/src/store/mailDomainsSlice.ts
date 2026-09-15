// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { MailDomain } from '../api/routes/mailRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSlice } from '@reduxjs/toolkit'

const mailDomainsAdapter = createEntityAdapter<MailDomain>({
  sortComparer: (first, second) => first.name.localeCompare(second.name),
})

export const mailDomainsSlice = createSlice({
  name: 'mailDomains',
  initialState: mailDomainsAdapter.getInitialState(),
  reducers: {
    mailDomainsLoaded: mailDomainsAdapter.setAll,
    mailDomainUpserted: mailDomainsAdapter.upsertOne,
    mailDomainDeleted(state, action: PayloadAction<string>) {
      mailDomainsAdapter.removeOne(state, action.payload)
    },
  },
})

export const { mailDomainsLoaded, mailDomainUpserted, mailDomainDeleted } = mailDomainsSlice.actions

export const {
  selectAll: selectAllMailDomains,
} = mailDomainsAdapter.getSelectors((state: RootState) => state.mailDomains)
