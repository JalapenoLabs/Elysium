// Copyright © 2026 Jalapeno Labs

// Core
import { createBrowserRouter } from 'react-router'

// UI
import { RootLayout } from './components/RootLayout'
import { AppLayout } from './components/AppLayout'
import { Home } from './pages/Home/Home'
import { About } from './pages/About/About'
import { SignIn } from './pages/Auth/SignIn'
import { SignedOut } from './pages/Auth/SignedOut'
import { ClaimInstance } from './pages/Setup/ClaimInstance'
import { Welcome } from './pages/Onboarding/Welcome'
import { ProfileStep } from './pages/Onboarding/ProfileStep'
import { InviteAcceptance } from './pages/Invite/InviteAcceptance'
import { SecurityCenter } from './pages/Settings/SecurityCenter'
import { ActiveSessions } from './pages/Settings/ActiveSessions'
import { ProfileSettings } from './pages/Settings/ProfileSettings'
import { AccessDenied } from './pages/Fault/AccessDenied'
import { Suspended } from './pages/Fault/Suspended'
import { IdentityUnavailable } from './pages/Fault/IdentityUnavailable'

// Misc
import { RequireAuth } from './auth/RequireAuth'
import { RequireInstance } from './auth/RequireInstance'
import { UrlTree } from './urls'

export const router = createBrowserRouter([
  {
    element: <RootLayout />,
    children: [
      // Full-screen auth screens: no app chrome. Public entry points and the
      // ceremony-adjacent flows the app owns (the credential steps themselves are
      // Keycloak's themed flow).
      { path: UrlTree.signIn, element: <SignIn /> },
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
        element: <RequireInstance><RequireAuth><AppLayout /></RequireAuth></RequireInstance>,
        children: [
          { path: UrlTree.home, element: <Home /> },
          { path: UrlTree.about, element: <About /> },
          { path: UrlTree.settingsSecurity, element: <SecurityCenter /> },
          { path: UrlTree.settingsSessions, element: <ActiveSessions /> },
          { path: UrlTree.settingsProfile, element: <ProfileSettings /> },
        ],
      },
    ],
  },
])
