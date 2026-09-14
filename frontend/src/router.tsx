// Copyright © 2026 Jalapeno Labs

// Core
import { createBrowserRouter, Navigate } from 'react-router'

// User interface
import { AppShell } from './layout/AppShell'
import { HomePage } from './pages/Home/HomePage'
import { SettingsDirectoryPage } from './pages/Settings/SettingsDirectoryPage'
import { PersonalDetailsPage } from './pages/Settings/PersonalDetails/PersonalDetailsPage'
import { ManageLlmsPage } from './pages/Settings/Llms/ManageLlmsPage'

// Misc
import { UNKNOWN_ROUTE_REDIRECT_TO, UrlTree } from './urls'

export const router = createBrowserRouter([
  {
    path: UrlTree.root,
    element: <AppShell />,
    children: [
      {
        index: true,
        element: <HomePage />,
      },
      {
        path: UrlTree.settings,
        element: <SettingsDirectoryPage />,
      },
      {
        path: UrlTree.settingsPersonalDetails,
        element: <PersonalDetailsPage />,
      },
      {
        path: UrlTree.settingsLlms,
        element: <ManageLlmsPage />,
      },
      {
        path: '*',
        element: <Navigate to={UNKNOWN_ROUTE_REDIRECT_TO} replace />,
      },
    ],
  },
])
