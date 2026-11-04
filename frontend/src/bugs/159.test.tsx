// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { Provider } from 'react-redux'
import { createMemoryRouter, Outlet, RouterProvider, useLocation, useParams } from 'react-router'

// Redux
import { createAppStore } from '../store'

// Misc
import { workspaceRoutes } from '../router'
import { UrlTree } from '../urls'

// Issue #159: every create flow is a page of its own under `/new`, not a dialog. Two of those
// pages share a level with a record's own page (`/studio/new` beside `/studio/:itemId`,
// `/coding/new` beside `/coding/:sessionId`), so `new` must never be read as an id. The
// addresses that opened a create flow before, `/coding?item=` and `/settings/llms/add-<slug>`,
// must still land on its page, so links and bookmarks to them keep working.

// Each page names itself, and what it was opened with, so the test sees where a path lands.
vi.mock('../pages/Studio/NewStudioItemPage', () => ({
  NewStudioItemPage: () => <p>New Studio item page</p>,
}))
vi.mock('../pages/Studio/StudioItemPage', () => ({
  StudioItemPage: () => <p>{`Studio item ${useParams().itemId}`}</p>,
}))
vi.mock('../pages/Coding/NewCodingSessionPage', () => ({
  NewCodingSessionPage: () => <p>{`New coding session page${useLocation().search}`}</p>,
}))
// The Coding page itself redirects `?item=`; it needs only the sessions' load state for that.
vi.mock('../hooks/useServerData', () => ({
  useCodingSessionsLoader: () => 'loading',
}))
vi.mock('../pages/Settings/Llms/AddLlmPage', () => ({
  AddLlmPage: () => <p>{`Add LLM page for ${useParams().provider}`}</p>,
}))
vi.mock('../pages/Settings/Llms/ManageLlmsPage', () => ({
  ManageLlmsPage: () => <p>LLM list</p>,
}))
vi.mock('../pages/Initiatives/NewInitiativeContainerPage', () => ({
  NewInitiativeContainerPage: () => <p>{`Follow page for ${useParams().initiativeId}`}</p>,
}))
vi.mock('../pages/Settings/Email/NewMailServerPage', () => ({
  NewMailServerPage: () => <p>New mail server page</p>,
}))
vi.mock('../pages/Settings/Email/NewMailDomainPage', () => ({
  NewMailDomainPage: () => <p>New mail domain page</p>,
}))
vi.mock('../pages/Settings/Email/NewMailboxPage', () => ({
  NewMailboxPage: () => <p>New mailbox page</p>,
}))
vi.mock('../pages/Settings/Email/ManageEmailPage', () => ({
  ManageEmailPage: () => <p>Email page</p>,
}))

// The workspace's own routes, without the app's frame around them.
function renderAt(path: string) {
  const router = createMemoryRouter([{ ...workspaceRoutes, element: <Outlet /> }], {
    initialEntries: [ path ],
  })
  render(<Provider store={createAppStore()}>
    <RouterProvider router={router} />
  </Provider>)
  return router
}

describe('issue #159', () => {
  it('opens New Studio item at /studio/new, never an item named "new"', async () => {
    renderAt(UrlTree.studioNew)
    expect(await screen.findByText('New Studio item page')).toBeTruthy()
  })

  it('still opens a Studio item by its id', async () => {
    renderAt('/studio/0193f2c1')
    expect(await screen.findByText('Studio item 0193f2c1')).toBeTruthy()
  })

  it('opens New session at /coding/new, never a session numbered "new"', async () => {
    renderAt(UrlTree.codingNew)
    expect(await screen.findByText('New coding session page')).toBeTruthy()
  })

  it('lands a link to /coding?item= on New session started from that item', async () => {
    const router = renderAt('/coding?item=item-1')

    expect(await screen.findByText('New coding session page?item=item-1')).toBeTruthy()
    expect(router.state.location.pathname).toBe('/coding/new')
  })

  it.each([
    [ 'claude-account' ],
    [ 'claude-api-key' ],
    [ 'claude-code-oauth' ],
    [ 'openai-api-key' ],
    [ 'codex-oauth' ],
  ])('lands /settings/llms/add-%s on its page under /settings/llms/new', async (slug) => {
    const router = renderAt(`/settings/llms/add-${slug}`)

    expect(await screen.findByText(`Add LLM page for ${slug}`)).toBeTruthy()
    expect(router.state.location.pathname).toBe(`/settings/llms/new/${slug}`)
  })

  it('sends /settings/llms/new, which names no provider, up to the list', async () => {
    const router = renderAt('/settings/llms/new')

    expect(await screen.findByText('LLM list')).toBeTruthy()
    expect(router.state.location.pathname).toBe(UrlTree.settingsLlms)
  })

  it('opens following a container on an initiative as its own page', async () => {
    renderAt('/action-items/initiatives/initiative-1/containers/new')
    expect(await screen.findByText('Follow page for initiative-1')).toBeTruthy()
  })

  it.each([
    [ UrlTree.settingsEmailServerNew, 'New mail server page' ],
    [ UrlTree.settingsEmailDomainNew, 'New mail domain page' ],
    [ UrlTree.settingsEmailMailboxNew, 'New mailbox page' ],
  ])('opens %s as its own page', async (path, page) => {
    renderAt(path)
    expect(await screen.findByText(page)).toBeTruthy()
  })

  it('sends an unknown address beside Email\'s create pages up to Email', async () => {
    const router = renderAt('/settings/email/domains/nothing')

    expect(await screen.findByText('Email page')).toBeTruthy()
    expect(router.state.location.pathname).toBe(UrlTree.settingsEmail)
  })
})
