// Copyright © 2026 Jalapeno Labs

// Core
import { createBrowserRouter, Navigate } from 'react-router'

// User interface
import { AppShell } from './layout/AppShell'
import { ActionItemListPage } from './pages/ActionItems/ActionItemListPage'
import { ActionItemPage } from './pages/ActionItems/ActionItemPage'
import { ActionItemsLayout } from './pages/ActionItems/ActionItemsLayout'
import { CreateActionItemPage } from './pages/ActionItems/CreateActionItemPage'
import { InboxPage } from './pages/ActionItems/InboxPage'
import { NextPage } from './pages/ActionItems/NextPage'
import { ChangesetPage } from './pages/Changesets/ChangesetPage'
import { ChangesetsPage } from './pages/Changesets/ChangesetsPage'
import { CodingPage } from './pages/Coding/CodingPage'
import { CreateInitiativePage } from './pages/Initiatives/CreateInitiativePage'
import { InitiativePage } from './pages/Initiatives/InitiativePage'
import { InitiativesPage } from './pages/Initiatives/InitiativesPage'
import { CreateProjectPage } from './pages/Projects/CreateProjectPage'
import { ProjectPage } from './pages/Projects/ProjectPage'
import { ProjectsPage } from './pages/Projects/ProjectsPage'
import { StudioPage } from './pages/Studio/StudioPage'
import { SettingsDirectoryPage } from './pages/Settings/SettingsDirectoryPage'
import { PersonalDetailsPage } from './pages/Settings/PersonalDetails/PersonalDetailsPage'
import { ManageEmailPage } from './pages/Settings/Email/ManageEmailPage'
import { AddEnvironmentVariablePage } from './pages/Settings/Environment/AddEnvironmentVariablePage'
import { EditEnvironmentVariablePage } from './pages/Settings/Environment/EditEnvironmentVariablePage'
import { ManageEnvironmentPage } from './pages/Settings/Environment/ManageEnvironmentPage'
import { AddGithubCredentialPage } from './pages/Settings/Github/AddGithubCredentialPage'
import { EditGithubCredentialPage } from './pages/Settings/Github/EditGithubCredentialPage'
import { ManageGithubPage } from './pages/Settings/Github/ManageGithubPage'
import { AddJiraCredentialPage } from './pages/Settings/Jira/AddJiraCredentialPage'
import { EditJiraCredentialPage } from './pages/Settings/Jira/EditJiraCredentialPage'
import { JiraDoneTransitionsPage } from './pages/Settings/Jira/JiraDoneTransitionsPage'
import { ManageJiraPage } from './pages/Settings/Jira/ManageJiraPage'
import { AddLlmPage } from './pages/Settings/Llms/AddLlmPage'
import { EditLlmPage } from './pages/Settings/Llms/EditLlmPage'
import { ManageLlmsPage } from './pages/Settings/Llms/ManageLlmsPage'
import { ManageSatellitesPage } from './pages/Settings/Satellites/ManageSatellitesPage'
import { AddStorageLocationPage } from './pages/Settings/Storage/AddStorageLocationPage'
import { EditStorageLocationPage } from './pages/Settings/Storage/EditStorageLocationPage'
import { ManageStoragePage } from './pages/Settings/Storage/ManageStoragePage'

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
        element: <Navigate to={UNKNOWN_ROUTE_REDIRECT_TO} replace />,
      },
      // Next, the inbox, every item, and initiatives share the area's heading and tabs.
      {
        path: UrlTree.actionItems,
        element: <ActionItemsLayout />,
        children: [
          {
            index: true,
            element: <NextPage />,
          },
          {
            path: UrlTree.actionItemsInbox,
            element: <InboxPage />,
          },
          {
            path: UrlTree.actionItemsAll,
            element: <ActionItemListPage />,
          },
          {
            path: UrlTree.initiatives,
            element: <InitiativesPage />,
          },
          {
            path: UrlTree.changesets,
            element: <ChangesetsPage />,
          },
        ],
      },
      {
        path: UrlTree.actionItemsNew,
        element: <CreateActionItemPage />,
      },
      {
        path: UrlTree.actionItemView,
        element: <ActionItemPage />,
      },
      {
        path: UrlTree.initiativesNew,
        element: <CreateInitiativePage />,
      },
      {
        path: UrlTree.initiativeView,
        element: <InitiativePage />,
      },
      {
        path: UrlTree.changesetView,
        element: <ChangesetPage />,
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
        path: UrlTree.codingSession,
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
        path: UrlTree.settingsEnvironment,
        element: <ManageEnvironmentPage />,
      },
      {
        path: UrlTree.settingsEnvironmentNew,
        element: <AddEnvironmentVariablePage />,
      },
      {
        path: UrlTree.settingsEnvironmentEdit,
        element: <EditEnvironmentVariablePage />,
      },
      {
        path: UrlTree.settingsGithub,
        element: <ManageGithubPage />,
      },
      {
        path: UrlTree.settingsGithubNew,
        element: <AddGithubCredentialPage />,
      },
      {
        path: UrlTree.settingsGithubEdit,
        element: <EditGithubCredentialPage />,
      },
      {
        path: UrlTree.settingsJira,
        element: <ManageJiraPage />,
      },
      {
        path: UrlTree.settingsJiraNew,
        element: <AddJiraCredentialPage />,
      },
      {
        path: UrlTree.settingsJiraEdit,
        element: <EditJiraCredentialPage />,
      },
      {
        path: UrlTree.settingsJiraDoneTransitions,
        element: <JiraDoneTransitionsPage />,
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
        path: UrlTree.settingsStorage,
        element: <ManageStoragePage />,
      },
      {
        path: UrlTree.settingsStorageNew,
        element: <AddStorageLocationPage />,
      },
      {
        path: UrlTree.settingsStorageEdit,
        element: <EditStorageLocationPage />,
      },
      {
        path: '*',
        element: <Navigate to={UNKNOWN_ROUTE_REDIRECT_TO} replace />,
      },
    ],
  },
])
