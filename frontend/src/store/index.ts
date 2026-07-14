// Copyright © 2026 Jalapeno Labs

// Core
import { configureStore } from '@reduxjs/toolkit'

// Redux
import { themeReducer } from './themeSlice'
import { sessionReducer } from './sessionSlice'
import { currentUserReducer } from './currentUserSlice'

export const store = configureStore({
  reducer: {
    theme: themeReducer,
    session: sessionReducer,
    currentUser: currentUserReducer,
  },
})

export type RootState = ReturnType<typeof store.getState>
export type AppDispatch = typeof store.dispatch

// The raw dispatch, for non-component code (API route modules, session helpers)
// that needs to dispatch without the `useAppDispatch` hook.
export const dispatch = store.dispatch
