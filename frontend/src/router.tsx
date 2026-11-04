// Copyright © 2026 Jalapeno Labs

import type { RouteObject } from 'react-router'

// Core
import { createBrowserRouter, Navigate } from 'react-router'

// User interface
import { AuthGate } from './auth/AuthGate'
import { BrowserSignInRedirect } from './pages/Settings/Connectors/BrowserSignInRedirect'
import { ConnectorPickerPage } from './pages/Settings/Connectors/ConnectorPickerPage'
import { ConnectorPage } from './pages/Settings/Connectors/ConnectorPage'
import { NewConnectorPage } from './pages/Settings/Connectors/NewConnectorPage'
import { ConnectorsPage } from './pages/Settings/Connectors/ConnectorsPage'
import { AppShell } from './layout/AppShell'
import { UnknownRouteRedirect } from './layout/UnknownRouteRedirect'
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
import { NewCodingSessionPage } from './pages/Coding/NewCodingSessionPage'
import { CreateInitiativePage } from './pages/Initiatives/CreateInitiativePage'
import { InitiativePage } from './pages/Initiatives/InitiativePage'
import { InitiativesPage } from './pages/Initiatives/InitiativesPage'
import { NewInitiativeContainerPage } from './pages/Initiatives/NewInitiativeContainerPage'
import { CreateProjectPage } from './pages/Projects/CreateProjectPage'
import { ProjectPage } from './pages/Projects/ProjectPage'
import { ProjectsPage } from './pages/Projects/ProjectsPage'
import { RunPage } from './pages/Runs/RunPage'
import { RunsPage } from './pages/Runs/RunsPage'
import { SearchPage } from './pages/Search/SearchPage'
import { NewStudioItemPage } from './pages/Studio/NewStudioItemPage'
import { StudioItemPage } from './pages/Studio/StudioItemPage'
import { StudioPage } from './pages/Studio/StudioPage'
import { SettingsDirectoryPage } from './pages/Settings/SettingsDirectoryPage'
import { AccountPage } from './pages/Settings/Account/AccountPage'
import { EditMailboxPage } from './pages/Settings/Email/EditMailboxPage'
import { ManageEmailPage } from './pages/Settings/Email/ManageEmailPage'
import { NewMailboxPage } from './pages/Settings/Email/NewMailboxPage'
import { NewMailDomainPage } from './pages/Settings/Email/NewMailDomainPage'
import { NewMailServerPage } from './pages/Settings/Email/NewMailServerPage'
import { AddEnvironmentVariablePage } from './pages/Settings/Environment/AddEnvironmentVariablePage'
import { EditEnvironmentVariablePage } from './pages/Settings/Environment/EditEnvironmentVariablePage'
import { ManageEnvironmentPage } from './pages/Settings/Environment/ManageEnvironmentPage'
import { EditGithubCredentialPage } from './pages/Settings/Github/EditGithubCredentialPage'
import { EditJiraCredentialPage } from './pages/Settings/Jira/EditJiraCredentialPage'
import { JiraDoneTransitionsPage } from './pages/Settings/Jira/JiraDoneTransitionsPage'
import { AddLlmPage } from './pages/Settings/Llms/AddLlmPage'
import { EditLlmPage } from './pages/Settings/Llms/EditLlmPage'
import { AgentConfigurationPage } from './pages/Settings/Llms/AgentConfigurationPage'
import { ManageLlmsPage } from './pages/Settings/Llms/ManageLlmsPage'
import { McpPage } from './pages/Settings/Mcp/McpPage'
import { AddTwoFactorPage } from './pages/Settings/Security/AddTwoFactorPage'
import { ChangeTwoFactorPage } from './pages/Settings/Security/ChangeTwoFactorPage'
import { RemoveTwoFactorPage } from './pages/Settings/Security/RemoveTwoFactorPage'
import { SecurityPage } from './pages/Settings/Security/SecurityPage'
import { ManageUsersPage } from './pages/Settings/Users/ManageUsersPage'
import { WorkspaceAccessPage } from './pages/Settings/Users/WorkspaceAccessPage'
import { AddStorageLocationPage } from './pages/Settings/Storage/AddStorageLocationPage'
import { EditStorageLocationPage } from './pages/Settings/Storage/EditStorageLocationPage'
import { ManageStoragePage } from './pages/Settings/Storage/ManageStoragePage'
import { UpdatesPage } from './pages/Settings/Updates/UpdatesPage'
import { BackupDestinationPage } from './pages/Settings/Backups/BackupDestinationPage'
import { BackupsPage } from './pages/Settings/Backups/BackupsPage'
import { PasswordsPage } from './pages/Settings/Passwords/PasswordsPage'
import { EditLearningCategoryPage } from './pages/Settings/LearningCategories/EditLearningCategoryPage'
import { LearningCategoriesPage } from './pages/Settings/LearningCategories/LearningCategoriesPage'
import { NewLearningCategoryPage } from './pages/Settings/LearningCategories/NewLearningCategoryPage'
import { LearningPage } from './pages/Settings/Learnings/LearningPage'
import { LearningsPage } from './pages/Settings/Learnings/LearningsPage'
import { MeetingsPage } from './pages/Settings/Meetings/MeetingsPage'
import { NewLearningPage } from './pages/Settings/Learnings/NewLearningPage'

// Misc
import { llmSlugByType } from './pages/Settings/Llms/llmProviders'
import { LLM_TYPES } from './pages/Settings/Llms/llmPresentation'
import { getNewLlmUrl, UNKNOWN_ROUTE_REDIRECT_TO, UrlTree } from './urls'

// How AppShell frames a route. `page` (the default) is a padded, scrolling column;
// `workspace` gives the page the whole content area and lets it manage its own
// scrolling, as the Coding page's Dockview layout needs.
export type RouteLayoutHandle = {
  layout: 'page' | 'workspace'
}

// Every workspace page, inside the app's frame. Exported for tests that follow its redirects.
export const workspaceRoutes: RouteObject = {
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
      path: UrlTree.initiativeContainerNew,
      element: <NewInitiativeContainerPage />,
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
    // Before the session's own route, so `new` is never read as a session's number.
    {
      path: UrlTree.codingNew,
      element: <NewCodingSessionPage />,
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
    // Before the item's own route, so `new` is never read as an item's id.
    {
      path: UrlTree.studioNew,
      element: <NewStudioItemPage />,
    },
    // The item page splits the whole content area between the stage and the conversation,
    // each scrolling on its own.
    {
      path: UrlTree.studioItem,
      element: <StudioItemPage />,
      handle: { layout: 'workspace' } satisfies RouteLayoutHandle,
    },
    {
      path: UrlTree.runs,
      element: <RunsPage />,
    },
    {
      path: UrlTree.runView,
      element: <RunPage />,
    },
    {
      path: UrlTree.search,
      element: <SearchPage />,
    },
    {
      path: UrlTree.settings,
      element: <SettingsDirectoryPage />,
    },
    {
      path: UrlTree.settingsAccount,
      element: <AccountPage />,
    },
    {
      path: UrlTree.settingsProfile,
      element: <Navigate to={UrlTree.settingsAccount} replace />,
    },
    {
      path: UrlTree.settingsPersonalDetails,
      element: <Navigate to={UrlTree.settingsAccount} replace />,
    },
    {
      path: UrlTree.settingsSecurity,
      element: <SecurityPage />,
    },
    {
      path: UrlTree.settingsTwoFactorNew,
      element: <AddTwoFactorPage />,
    },
    {
      path: UrlTree.settingsTwoFactorEdit,
      element: <ChangeTwoFactorPage />,
    },
    {
      path: UrlTree.settingsTwoFactorRemove,
      element: <RemoveTwoFactorPage />,
    },
    // A browser is a connector's account, shown on the connector's page.
    {
      path: UrlTree.settingsBrowsers,
      element: <Navigate to={UrlTree.settingsConnectors} replace />,
    },
    {
      path: UrlTree.settingsBrowser,
      element: <BrowserSignInRedirect />,
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
      path: UrlTree.settingsUsersAccess,
      element: <WorkspaceAccessPage />,
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
      path: UrlTree.settingsBackupsDestination,
      element: <BackupDestinationPage />,
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
      path: UrlTree.settingsLearnings,
      element: <LearningsPage />,
    },
    {
      path: UrlTree.settingsLearningNew,
      element: <NewLearningPage />,
    },
    {
      path: UrlTree.settingsLearning,
      element: <LearningPage />,
    },
    {
      path: UrlTree.settingsLearningCategories,
      element: <LearningCategoriesPage />,
    },
    {
      path: UrlTree.settingsLearningCategoryNew,
      element: <NewLearningCategoryPage />,
    },
    {
      path: UrlTree.settingsLearningCategoryEdit,
      element: <EditLearningCategoryPage />,
    },
    {
      path: UrlTree.settingsLlms,
      element: <ManageLlmsPage />,
    },
    {
      path: UrlTree.settingsLlmNew,
      element: <AddLlmPage />,
    },
    // The add pages' earlier addresses, `add-<slug>`, so links and bookmarks to them still land.
    ...LLM_TYPES.map((type) => ({
      path: `${UrlTree.settingsLlms}/add-${llmSlugByType[type]}`,
      element: <Navigate to={getNewLlmUrl(llmSlugByType[type])} replace />,
    })),
    {
      path: UrlTree.settingsLlmsEdit,
      element: <EditLlmPage />,
    },
    {
      path: UrlTree.settingsLlmsAgentConfiguration,
      element: <AgentConfigurationPage />,
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
      element: <ConnectorPickerPage />,
    },
    {
      path: UrlTree.settingsConnectorNewKind,
      element: <NewConnectorPage />,
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
      path: UrlTree.settingsEmailServerNew,
      element: <NewMailServerPage />,
    },
    {
      path: UrlTree.settingsEmailDomainNew,
      element: <NewMailDomainPage />,
    },
    {
      path: UrlTree.settingsEmailMailboxNew,
      element: <NewMailboxPage />,
    },
    {
      path: UrlTree.settingsMailboxEdit,
      element: <EditMailboxPage />,
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
    // An address no route matches goes one level up, to its nearest parent page.
    {
      path: '*',
      element: <UnknownRouteRedirect />,
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
