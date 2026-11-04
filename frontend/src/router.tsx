// Copyright © 2026 Jalapeno Labs

import type { RouteObject } from 'react-router'

// Core
import { createBrowserRouter, Navigate } from 'react-router'

// User interface
import { AuthGate } from './auth/AuthGate'
import { AddConnectorPage } from './pages/Settings/Connectors/AddConnectorPage'
import { ConnectorPage } from './pages/Settings/Connectors/ConnectorPage'
import { ConnectorsPage } from './pages/Settings/Connectors/ConnectorsPage'
import { AppShell } from './layout/AppShell'
import { AuthErrorPage } from './pages/Auth/AuthErrorPage'
import { AuthLayout } from './pages/Auth/AuthLayout'
import { LoginPage } from './pages/Auth/LoginPage'
import { PendingPage } from './pages/Auth/PendingPage'
import { RecoveryPage } from './pages/Auth/RecoveryPage'
import { SignupPage } from './pages/Auth/SignupPage'
import { OAuthConsentPage } from './pages/OAuth/OAuthConsentPage'
import { OAuthLoginPage } from './pages/OAuth/OAuthLoginPage'
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
import { StudioItemPage } from './pages/Studio/StudioItemPage'
import { StudioPage } from './pages/Studio/StudioPage'
import { SettingsDirectoryPage } from './pages/Settings/SettingsDirectoryPage'
import { ProfilePage } from './pages/Settings/Profile/ProfilePage'
import { ManageEmailPage } from './pages/Settings/Email/ManageEmailPage'
import { AddEnvironmentVariablePage } from './pages/Settings/Environment/AddEnvironmentVariablePage'
import { EditEnvironmentVariablePage } from './pages/Settings/Environment/EditEnvironmentVariablePage'
import { ManageEnvironmentPage } from './pages/Settings/Environment/ManageEnvironmentPage'
import { EditGithubCredentialPage } from './pages/Settings/Github/EditGithubCredentialPage'
import { EditJiraCredentialPage } from './pages/Settings/Jira/EditJiraCredentialPage'
import { JiraDoneTransitionsPage } from './pages/Settings/Jira/JiraDoneTransitionsPage'
import { AddLlmPage } from './pages/Settings/Llms/AddLlmPage'
import { EditLlmPage } from './pages/Settings/Llms/EditLlmPage'
import { ManageLlmsPage } from './pages/Settings/Llms/ManageLlmsPage'
import { BrowserSessionPage } from './pages/Settings/Browsers/BrowserSessionPage'
import { ManageBrowsersPage } from './pages/Settings/Browsers/ManageBrowsersPage'
import { McpPage } from './pages/Settings/Mcp/McpPage'
import { SecurityPage } from './pages/Settings/Security/SecurityPage'
import { ManageUsersPage } from './pages/Settings/Users/ManageUsersPage'
import { AddStorageLocationPage } from './pages/Settings/Storage/AddStorageLocationPage'
import { EditStorageLocationPage } from './pages/Settings/Storage/EditStorageLocationPage'
import { ManageStoragePage } from './pages/Settings/Storage/ManageStoragePage'
import { UpdatesPage } from './pages/Settings/Updates/UpdatesPage'
import { BackupsPage } from './pages/Settings/Backups/BackupsPage'
import { PasswordsPage } from './pages/Settings/Passwords/PasswordsPage'
import { MeetingsPage } from './pages/Settings/Meetings/MeetingsPage'

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

// Every workspace page, inside the app's frame.
const workspaceRoutes: RouteObject = {
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
      path: UrlTree.studio,
      element: <StudioPage />,
    },
    // The item page splits the whole content area between the stage and the conversation,
    // each scrolling on its own.
    {
      path: UrlTree.studioItem,
      element: <StudioItemPage />,
      handle: { layout: 'workspace' } satisfies RouteLayoutHandle,
    },
    {
      path: UrlTree.settings,
      element: <SettingsDirectoryPage />,
    },
    {
      path: UrlTree.settingsProfile,
      element: <ProfilePage />,
    },
    {
      path: UrlTree.settingsPersonalDetails,
      element: <Navigate to={UrlTree.settingsProfile} replace />,
    },
    {
      path: UrlTree.settingsSecurity,
      element: <SecurityPage />,
    },
    {
      path: UrlTree.settingsBrowsers,
      element: <ManageBrowsersPage />,
    },
    {
      path: UrlTree.settingsBrowser,
      element: <BrowserSessionPage />,
    },
    {
      path: UrlTree.settingsMcp,
      element: <McpPage />,
    },
    {
      path: UrlTree.settingsConnectedApps,
      element: <Navigate to={UrlTree.settingsMcp} replace />,
    },
    {
      path: UrlTree.settingsUsers,
      element: <ManageUsersPage />,
    },
    {
      path: UrlTree.settingsUpdates,
      element: <UpdatesPage />,
    },
    {
      path: UrlTree.settingsBackups,
      element: <BackupsPage />,
    },
    {
      path: UrlTree.settingsPasswords,
      element: <PasswordsPage />,
    },
    {
      path: UrlTree.settingsMeetings,
      element: <MeetingsPage />,
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
    // The hub lists GitHub and Jira with every other connector, and adds them there.
    {
      path: UrlTree.settingsConnectors,
      element: <ConnectorsPage />,
    },
    {
      path: UrlTree.settingsConnectorNew,
      element: <AddConnectorPage />,
    },
    {
      path: UrlTree.settingsConnector,
      element: <ConnectorPage />,
    },
    {
      path: UrlTree.settingsGithub,
      element: <Navigate to={UrlTree.settingsConnectors} replace />,
    },
    {
      path: UrlTree.settingsGithubNew,
      element: <Navigate to={UrlTree.settingsConnectorNew} replace />,
    },
    {
      path: UrlTree.settingsGithubEdit,
      element: <EditGithubCredentialPage />,
    },
    {
      path: UrlTree.settingsJira,
      element: <Navigate to={UrlTree.settingsConnectors} replace />,
    },
    {
      path: UrlTree.settingsJiraNew,
      element: <Navigate to={UrlTree.settingsConnectorNew} replace />,
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
}

// Two trees: the pages anyone may open (signing in, signing up, recovery, waiting for
// approval), and what `AuthGate` opens only to people allowed in: the workspace, and the pages
// that connect an MCP client.
export const router = createBrowserRouter([
  {
    element: <AuthLayout />,
    children: [
      {
        path: UrlTree.login,
        element: <LoginPage />,
      },
      {
        path: UrlTree.signup,
        element: <SignupPage />,
      },
      {
        path: UrlTree.recovery,
        element: <RecoveryPage />,
      },
      {
        path: UrlTree.pending,
        element: <PendingPage />,
      },
      {
        path: UrlTree.authError,
        element: <AuthErrorPage />,
      },
    ],
  },
  {
    element: <AuthGate />,
    children: [
      // Connecting an MCP client: the account pages' look, behind the gate, so only someone
      // signed in and approved can answer for it.
      {
        element: <AuthLayout />,
        children: [
          {
            path: UrlTree.oauthLogin,
            element: <OAuthLoginPage />,
          },
          {
            path: UrlTree.oauthConsent,
            element: <OAuthConsentPage />,
          },
        ],
      },
      workspaceRoutes,
    ],
  },
])
