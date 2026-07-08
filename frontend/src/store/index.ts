// Copyright © 2026 Jalapeno Labs

// Core
import { configureStore } from '@reduxjs/toolkit'

// Redux
import { themeReducer } from './themeSlice'
import { counterReducer } from './counterSlice'
import { sessionReducer } from './sessionSlice'

export const store = configureStore({
  reducer: {
    theme: themeReducer,
    counter: counterReducer,
    session: sessionReducer,
  },
})

export type RootState = ReturnType<typeof store.getState>
export type AppDispatch = typeof store.dispatch

// The raw dispatch, for non-component code (API route modules, session helpers)
// that needs to dispatch without the `useAppDispatch` hook.
export const dispatch = store.dispatch
