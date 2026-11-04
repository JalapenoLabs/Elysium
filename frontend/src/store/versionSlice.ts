// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { RootState } from './index'

// Core
import { createSlice } from '@reduxjs/toolkit'

// Redux
import { updatesStatusUpdated } from './updatesSlice'

// Which build of Elysium this page runs, and which one the API runs now. A build is named by
// the time it was built (`builtAt` in `/api/version`): the image carries the API and the web
// app together, and every image built from a new commit gets a new one, on `main` as on a
// release, whereas `release` is null on `main` and a commit is missing from some builds.
type VersionState = {
  // The build the API ran when this page loaded, so the one whose web app it is running.
  // Null until it is read.
  loadedBuild: string | null
  // The build the API runs now, as last read.
  runningBuild: string | null
  // A build the person chose not to refresh for. A later build offers the refresh again.
  dismissedBuild: string | null
}

const initialState: VersionState = {
  loadedBuild: null,
  runningBuild: null,
  dismissedBuild: null,
}

export const versionSlice = createSlice({
  name: 'version',
  initialState,
  reducers: {
    // `/api/version` answered, on load or after the event stream reconnected. The first
    // answer is the build this page loaded.
    buildRead(state, action: PayloadAction<string>) {
      state.loadedBuild ??= action.payload
      state.runningBuild = action.payload
    },
    refreshPromptDismissed(state) {
      state.dismissedBuild = state.runningBuild
    },
  },
  extraReducers: (builder) => {
    // Auto-update's status names the build of the process that sent it, which runs the
    // same image as the API. It is only compared once the loaded build is known.
    builder.addCase(updatesStatusUpdated, (state, action) => {
      if (state.loadedBuild) {
        state.runningBuild = action.payload.builtAt
      }
    })
  },
})

export const { buildRead, refreshPromptDismissed } = versionSlice.actions

// Whether a build other than this page's runs now, and the person has not set it aside.
export function selectIsRefreshOffered(state: RootState) {
  const { loadedBuild, runningBuild, dismissedBuild } = state.version
  return Boolean(loadedBuild && runningBuild)
    && runningBuild !== loadedBuild
    && runningBuild !== dismissedBuild
}
