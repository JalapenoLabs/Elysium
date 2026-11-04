// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { CompanyDoc } from '../api/routes/companyDocRoutes'
import type { RootState } from './index'

// Core
import { createSlice } from '@reduxjs/toolkit'

type CompanyDocSliceState = {
  // Null until a member loads it.
  doc: CompanyDoc | null
}

const initialState: CompanyDocSliceState = {
  doc: null,
}

// The company doc, each section with its pending proposals. It is one record, answered whole by
// every read and write, so each answer replaces it; `companyDoc.changed` reloads it.
export const companyDocSlice = createSlice({
  name: 'companyDoc',
  initialState,
  reducers: {
    companyDocLoaded(state, action: PayloadAction<CompanyDoc>) {
      state.doc = action.payload
    },
  },
})

export const { companyDocLoaded } = companyDocSlice.actions

export function selectCompanyDoc(state: RootState) {
  return state.companyDoc.doc
}
