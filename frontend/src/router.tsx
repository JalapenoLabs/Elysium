// Copyright © 2026 Jalapeno Labs

// Core
import { createBrowserRouter, Navigate } from 'react-router'

// User interface
import { AppShell } from './layout/AppShell'
import { ActionItemsPage } from './pages/ActionItems/ActionItemsPage'
import { CodingPage } from './pages/Coding/CodingPage'
import { HomePage } from './pages/Home/HomePage'
import { CreateProjectPage } from './pages/Projects/CreateProjectPage'
import { ProjectPage } from './pages/Projects/ProjectPage'
import { ProjectsPage } from './pages/Projects/ProjectsPage'
import { StudioPage } from './pages/Studio/StudioPage'
import { SettingsDirectoryPage } from './pages/Settings/SettingsDirectoryPage'
import { PersonalDetailsPage } from './pages/Settings/PersonalDetails/PersonalDetailsPage'
import { ManageEmailPage } from './pages/Settings/Email/ManageEmailPage'
import { AddLlmPage } from './pages/Settings/Llms/AddLlmPage'
import { EditLlmPage } from './pages/Settings/Llms/EditLlmPage'
import { ManageLlmsPage } from './pages/Settings/Llms/ManageLlmsPage'
import { ManageSatellitesPage } from './pages/Settings/Satellites/ManageSatellitesPage'

// Misc
import { addLlmUrlByType } from './pages/Settings/Llms/llmProviders'
import { LLM_TYPES } from './pages/Settings/Llms/llmPresentation'
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
        path: UrlTree.actionItems,
        element: <ActionItemsPage />,
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
        path: UrlTree.projectsNew,
        element: <CreateProjectPage />,
      },
      {
        path: UrlTree.projectView,
        element: <ProjectPage />,
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
      ...LLM_TYPES.map((type) => ({
        path: addLlmUrlByType[type],
        element: <AddLlmPage type={type} />,
      })),
      {
        path: UrlTree.settingsLlmsEdit,
        element: <EditLlmPage />,
      },
      {
        path: UrlTree.settingsSatellites,
        element: <ManageSatellitesPage />,
      },
      {
        path: UrlTree.settingsEmail,
        element: <ManageEmailPage />,
      },
      {
        path: '*',
        element: <Navigate to={UNKNOWN_ROUTE_REDIRECT_TO} replace />,
      },
    ],
  },
])
