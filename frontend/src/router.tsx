// Copyright © 2026 Jalapeno Labs

// Core
import { createBrowserRouter, Navigate } from 'react-router'

// UI
import { RootLayout } from './components/RootLayout'
import { AppLayout } from './components/AppLayout'
import { Home } from './pages/Home/Home'
import { About } from './pages/About/About'
import { Login } from './pages/Auth/Login'
import { SignedOut } from './pages/Auth/SignedOut'
import { ClaimInstance } from './pages/Setup/ClaimInstance'
import { Welcome } from './pages/Onboarding/Welcome'
import { ProfileStep } from './pages/Onboarding/ProfileStep'
import { InviteAcceptance } from './pages/Invite/InviteAcceptance'
import { SettingsLayout } from './pages/Settings/SettingsLayout'
import { ProfileSettings } from './pages/Settings/ProfileSettings'
import { SecurityCenter } from './pages/Settings/SecurityCenter'
import { ActiveSessions } from './pages/Settings/ActiveSessions'
import { OrgSettingsLayout } from './pages/Organization/OrgSettingsLayout'
import { OrgGeneral } from './pages/Organization/OrgGeneral'
import { OrgMembers } from './pages/Organization/OrgMembers'
import { Teams } from './pages/Organization/Teams'
import { OrgRoles } from './pages/Organization/OrgRoles'
import { OrgSecurity } from './pages/Organization/OrgSecurity'
import { OrgAuditLog } from './pages/Organization/OrgAuditLog'
import { AccessDenied } from './pages/Fault/AccessDenied'
import { Suspended } from './pages/Fault/Suspended'
import { IdentityUnavailable } from './pages/Fault/IdentityUnavailable'

// Misc
import { RequireAuth } from './auth/RequireAuth'
import { RequireInstance } from './auth/RequireInstance'
import { RequireTotpEnrollment } from './auth/RequireTotpEnrollment'
import { UrlTree } from './urls'

export const router = createBrowserRouter([
  {
    element: <RootLayout />,
    children: [
      // Full-screen auth screens: no app chrome. Public entry points and the
      // ceremony flows the app owns (the SPA drives the native passkey prompt
      // against the API).
      { path: UrlTree.login, element: <Login /> },
      { path: UrlTree.legacySignIn, element: <Navigate to={UrlTree.login} replace /> },
      { path: UrlTree.signedOut, element: <SignedOut /> },
      { path: UrlTree.setup, element: <ClaimInstance /> },
      { path: UrlTree.onboarding, element: <Welcome /> },
      { path: UrlTree.onboardingProfile, element: <ProfileStep /> },
      { path: UrlTree.invite, element: <InviteAcceptance /> },
      { path: UrlTree.accessDenied, element: <AccessDenied /> },
      { path: UrlTree.suspended, element: <Suspended /> },
      { path: UrlTree.identityUnavailable, element: <IdentityUnavailable /> },

      // Protected in-app pages: the NavBar chrome, gated on a claimed instance
      // and an authenticated session.
      {
        element:
          <RequireInstance>
            <RequireAuth>
              <RequireTotpEnrollment>
                <AppLayout />
              </RequireTotpEnrollment>
            </RequireAuth>
          </RequireInstance>,
        children: [
          { path: UrlTree.home, element: <Home /> },
          { path: UrlTree.about, element: <About /> },
          {
            path: UrlTree.settings,
            element: <SettingsLayout />,
            children: [
              { index: true, element: <Navigate to={UrlTree.settingsProfile} replace /> },
              { path: UrlTree.settingsProfile, element: <ProfileSettings /> },
              { path: UrlTree.settingsSecurity, element: <SecurityCenter /> },
              { path: UrlTree.settingsSessions, element: <ActiveSessions /> },
            ],
          },
          {
            path: UrlTree.organization,
            element: <OrgSettingsLayout />,
            children: [
              { index: true, element: <Navigate to={UrlTree.organizationGeneral} replace /> },
              { path: UrlTree.organizationGeneral, element: <OrgGeneral /> },
              { path: UrlTree.organizationMembers, element: <OrgMembers /> },
              { path: UrlTree.organizationTeams, element: <Teams /> },
              { path: UrlTree.organizationRoles, element: <OrgRoles /> },
              { path: UrlTree.organizationSecurity, element: <OrgSecurity /> },
              { path: UrlTree.organizationAudit, element: <OrgAuditLog /> },
            ],
          },
        ],
      },
    ],
  },
])
