// Copyright © 2026 Jalapeno Labs

// Core
import { createBrowserRouter, Navigate } from 'react-router'

// User interface
import { AppShell } from './layout/AppShell'
import { CodingPage } from './pages/Coding/CodingPage'
import { HomePage } from './pages/Home/HomePage'
import { ProjectsPage } from './pages/Projects/ProjectsPage'
import { StudioPage } from './pages/Studio/StudioPage'
import { SettingsDirectoryPage } from './pages/Settings/SettingsDirectoryPage'
import { PersonalDetailsPage } from './pages/Settings/PersonalDetails/PersonalDetailsPage'
import { ManageLlmsPage } from './pages/Settings/Llms/ManageLlmsPage'
import { ManageSatellitesPage } from './pages/Settings/Satellites/ManageSatellitesPage'

// Misc
import { UNKNOWN_ROUTE_REDIRECT_TO, UrlTree } from './urls'

// How AppShell frames a route. `page` (the default) is a padded, scrolling column;
// `workspace` gives the page the whole content area and lets it manage its own
// scrolling, as the Coding page's Dockview layout needs.
export type RouteLayoutHandle = {
  layout: 'page' | 'workspace'
}

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
        path: UrlTree.studio,
        element: <StudioPage />,
      },
      {
        path: UrlTree.projects,
        element: <ProjectsPage />,
      },
      {
        path: UrlTree.coding,
        element: <CodingPage />,
        handle: { layout: 'workspace' } satisfies RouteLayoutHandle,
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
        path: UrlTree.settingsSatellites,
        element: <ManageSatellitesPage />,
      },
      {
        path: '*',
        element: <Navigate to={UNKNOWN_ROUTE_REDIRECT_TO} replace />,
      },
    ],
  },
])
