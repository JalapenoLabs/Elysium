// Copyright © 2026 Jalapeno Labs

// Single source of truth for in-app navigation targets.
export const UrlTree = {
  home: '/',
  about: '/about',
} as const

export type UrlValue = typeof UrlTree[keyof typeof UrlTree]

export const ExternalLinks = {
  heroui: 'https://www.heroui.com',
  vite: 'https://vite.dev',
  reactRouter: 'https://reactrouter.com',
} as const

export type ExternalLinkValue = typeof ExternalLinks[keyof typeof ExternalLinks]
