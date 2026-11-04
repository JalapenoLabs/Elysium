// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'
import { render, screen } from '@testing-library/react'
import { MemoryRouter, useNavigate } from 'react-router'
import { useRouterHref } from './useRouterHref'

// User interface
import { RouterProvider as AriaRouterProvider, Link } from '@heroui/react'

// The router frame AppShell and AuthLayout give every HeroUI link.
function RoutedLinks() {
  const navigate = useNavigate()
  return <AriaRouterProvider navigate={navigate} useHref={useRouterHref}>
    <Link href='/settings/github'>GitHub tokens</Link>
    <Link href='https://github.com/JalapenoLabs/Elysium/issues/164' target='_blank'>The issue</Link>
    <Link href='mailto:alex@example.com'>Mail</Link>
  </AriaRouterProvider>
}

describe('useRouterHref', () => {
  it('keeps a link to another site at its own address, and routes the rest in the app', () => {
    render(<MemoryRouter basename='/app' initialEntries={[ '/app/settings' ]}>
      <RoutedLinks />
    </MemoryRouter>)

    expect(screen.getByRole('link', { name: 'GitHub tokens' }).getAttribute('href')).toBe('/app/settings/github')
    expect(screen.getByRole('link', { name: 'The issue' }).getAttribute('href'))
      .toBe('https://github.com/JalapenoLabs/Elysium/issues/164')
    expect(screen.getByRole('link', { name: 'Mail' }).getAttribute('href')).toBe('mailto:alex@example.com')
  })
})
