// Copyright © 2026 Jalapeno Labs

// Single source of truth for in-app navigation targets. All navigation resolves
// through here rather than hardcoding paths across the app.
export const UrlTree = {
  home: '/',
  about: '/about',

  // Sign-in ceremony (the SPA drives the native passkey prompt against the API).
  login: '/login',
  // The old sign-in path stays routable as a redirect so bookmarks and links
  // minted before the rename still land on the login screen.
  legacySignIn: '/sign-in',
  signedOut: '/signed-out',

  // Bootstrap & onboarding.
  setup: '/setup',
  onboarding: '/onboarding',
  onboardingProfile: '/onboarding/profile',

  // Invitations (signed link).
  invite: '/invite/:token',

  // Personal account settings: the simple, flat-sidebar hub for "me" concerns.
  settings: '/settings',
  settingsProfile: '/settings/profile',
  settingsSecurity: '/settings/security',
  settingsSessions: '/settings/sessions',

  // Organization settings: a separate hub with an accordion sidebar for
  // org-level concerns (identity, branding, people).
  organization: '/organization',
  organizationGeneral: '/organization/general',
  organizationProjects: '/organization/projects',
  organizationStorage: '/organization/storage',
  organizationIntegrations: '/organization/integrations',
  organizationMembers: '/organization/members',
  organizationTeams: '/organization/teams',
  organizationRoles: '/organization/roles',
  organizationSecurity: '/organization/security',
  organizationAudit: '/organization/audit',

  // Denied & fault states.
  accessDenied: '/denied',
  suspended: '/suspended',
  identityUnavailable: '/identity-unavailable',
} as const

export type UrlValue = typeof UrlTree[keyof typeof UrlTree]

export const ExternalLinks = {
  heroui: 'https://www.heroui.com',
  vite: 'https://vite.dev',
  reactRouter: 'https://reactrouter.com',
} as const

export type ExternalLinkValue = typeof ExternalLinks[keyof typeof ExternalLinks]

// Builds the acceptance URL for a signed invite token.
export const getInviteUrl = (token: string) =>
  UrlTree.invite.replace(':token', token)

// Login in "add account" mode: skips the authenticated redirect so a signed-in
// user can authenticate an additional account (see the profile badge menu).
export const getAddAccountUrl = () =>
  `${UrlTree.login}?intent=add`
