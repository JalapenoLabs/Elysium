// Copyright © 2026 Jalapeno Labs

// Core
import { configureStore } from '@reduxjs/toolkit'

// Redux
import { themeReducer } from './themeSlice'
import { counterReducer } from './counterSlice'

export const store = configureStore({
  reducer: {
    theme: themeReducer,
    counter: counterReducer,
  },
})

export type RootState = ReturnType<typeof store.getState>
export type AppDispatch = typeof store.dispatch
