// Copyright © 2026 Jalapeno Labs

import type { KnowledgePage, PageAttachment, PageAttachmentPolicy } from '../../../api/routes/pageRoutes'

// Core
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { SWRConfig } from 'swr'
import { beforeEach, describe, expect, it, vi } from 'vitest'

// Redux
import { createAppStore } from '../../../store'
import { usersLoaded } from '../../../store/usersSlice'

// User interface
import { PageAttachments } from './PageAttachments'

// Misc
import {
  attachPageFile,
  getPageAttachmentPolicy,
  removePageAttachment,
  restorePageAttachment,
} from '../../../api/routes/pageRoutes'
import { makePage, makeUser } from '../../../testFixtures'
import { TestAriaRouter } from '../../../testRouter'
import { toast } from '../../../toast/toast'
import { usePageAttachmentUpload } from './usePageAttachments'

vi.mock('../../../api/routes/pageRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../../api/routes/pageRoutes')>(),
  attachPageFile: vi.fn(),
  getPageAttachmentPolicy: vi.fn(),
  removePageAttachment: vi.fn(),
  restorePageAttachment: vi.fn(),
}))

vi.mock('../../../toast/toast', () => ({
  toast: { success: vi.fn(), danger: vi.fn() },
}))

const POLICY: PageAttachmentPolicy = {
  destinationId: 'location',
  maxBytes: 50_000_000,
  extensions: [ 'png', 'pdf' ],
}

function makeAttachment(overrides: Partial<PageAttachment> = {}): PageAttachment {
  return {
    id: 'attachment',
    pageId: 'page',
    name: 'plan.pdf',
    url: '/api/v1/pages/page/attachments/attachment/plan.pdf',
    sizeBytes: 1_500_000,
    isImage: false,
    createdBy: 'user-1',
    agent: null,
    createdAt: '2026-11-01T12:00:00Z',
    ...overrides,
  }
}

function Section(props: { page: KnowledgePage }) {
  const upload = usePageAttachmentUpload(props.page.id)
  return <PageAttachments page={props.page} upload={upload} />
}

function renderAttachments(page: KnowledgePage) {
  const store = createAppStore()
  store.dispatch(usersLoaded([ makeUser({ id: 'user-1', name: 'Alex Navarro' }) ]))
  const router = createMemoryRouter([{
    path: '*',
    element: <SWRConfig value={{ provider: () => new Map() }}>
      <Provider store={store}>
        <TestAriaRouter>
          <Section page={page} />
        </TestAriaRouter>
      </Provider>
    </SWRConfig>,
  }])
  render(<RouterProvider router={router} />)
  return store
}

function drop(target: Element, files: File[]) {
  const dataTransfer = { types: [ 'Files' ], files }
  fireEvent.dragOver(target, { dataTransfer })
  fireEvent.drop(target, { dataTransfer })
}

describe('PageAttachments', () => {
  beforeEach(() => {
    vi.mocked(attachPageFile).mockReset()
    vi.mocked(removePageAttachment).mockReset()
    vi.mocked(restorePageAttachment).mockReset()
    vi.mocked(toast.success).mockReset()
    vi.mocked(toast.danger).mockReset()
    vi.mocked(getPageAttachmentPolicy).mockReset()
    vi.mocked(getPageAttachmentPolicy).mockResolvedValue({ policy: POLICY })
  })

  it('lists every file with its size, who added it, and when', async () => {
    renderAttachments(makePage({ id: 'page', attachments: [ makeAttachment() ]}))

    const link = await screen.findByRole('link', { name: 'Download plan.pdf' })

    expect(link.getAttribute('href')).toBe('/api/v1/pages/page/attachments/attachment/plan.pdf')
    expect(link.hasAttribute('download')).toBe(true)
    expect(screen.getByText('1.5 MB · Alex Navarro')).toBeTruthy()
    expect(screen.getByText(/November 1st/u)).toBeTruthy()
  })

  it('shows an image as itself', async () => {
    const attachment = makeAttachment({ name: 'plan.png', isImage: true, url: '/api/plan.png' })
    const { container } = render(<div />)
    renderAttachments(makePage({ id: 'page', attachments: [ attachment ]}))

    await screen.findByRole('link', { name: 'Download plan.png' })

    expect(container.ownerDocument.querySelector('img')?.getAttribute('src')).toBe('/api/plan.png')
  })

  it('says who chooses where files go before anyone tries to attach one', async () => {
    vi.mocked(getPageAttachmentPolicy).mockResolvedValue({ policy: { ...POLICY, destinationId: null }})
    renderAttachments(makePage({ id: 'page' }))

    expect(await screen.findByText('An admin chooses where attachments are kept, in Settings, Storage.')).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Attach files' })).toBeNull()
  })

  it('attaches what is dropped on it, one file at a time', async () => {
    const page = makePage({ id: 'page' })
    const attachment = makeAttachment()
    vi.mocked(attachPageFile).mockResolvedValue({ attachment, page: { ...page, attachments: [ attachment ]}})
    const store = renderAttachments(page)
    const section = await screen.findByRole('region', { name: 'Attachments' })
    await screen.findByRole('button', { name: 'Attach files' })
    const first = new File([ 'one' ], 'one.pdf')
    const second = new File([ 'two' ], 'two.pdf')

    drop(section, [ first, second ])

    await waitFor(() => expect(attachPageFile).toHaveBeenCalledTimes(2))
    expect(vi.mocked(attachPageFile).mock.calls.map(([ , file ]) => file)).toEqual([ first, second ])
    await waitFor(() => {
      expect(store.getState().pages.openedById.page?.attachments).toEqual([ attachment ])
    })
  })

  it('refuses a file the policy refuses, saying why, and still sends the ones beside it', async () => {
    const page = makePage({ id: 'page' })
    const attachment = makeAttachment()
    vi.mocked(attachPageFile).mockResolvedValue({ attachment, page })
    renderAttachments(page)
    const section = await screen.findByRole('region', { name: 'Attachments' })
    await screen.findByRole('button', { name: 'Attach files' })

    drop(section, [ new File([ 'x' ], 'tool.exe'), new File([ 'x' ], 'plan.pdf') ])

    await waitFor(() => expect(attachPageFile).toHaveBeenCalledTimes(1))
    expect(toast.danger).toHaveBeenCalledWith('tool.exe wasn\'t attached', {
      description: 'Pages don\'t take .exe files.',
    })
  })

  it('removes a file at once, and its toast puts it back', async () => {
    const attachment = makeAttachment()
    const page = makePage({ id: 'page', attachments: [ attachment ]})
    vi.mocked(removePageAttachment).mockResolvedValue({ page: { ...page, attachments: []}})
    vi.mocked(restorePageAttachment).mockResolvedValue({ page })
    const store = renderAttachments(page)

    fireEvent.click(await screen.findByRole('button', { name: 'Remove plan.pdf' }))

    await waitFor(() => expect(toast.success).toHaveBeenCalled())
    expect(removePageAttachment).toHaveBeenCalledWith('page', 'attachment')
    expect(store.getState().pages.openedById.page?.attachments).toEqual([])

    const [ message, options ] = vi.mocked(toast.success).mock.calls[0]
    expect(message).toBe('Removed plan.pdf')
    options?.action?.onPress()

    await waitFor(() => expect(restorePageAttachment).toHaveBeenCalledWith('page', 'attachment'))
    await waitFor(() => {
      expect(store.getState().pages.openedById.page?.attachments).toEqual([ attachment ])
    })
  })

  it('only lists what a deleted page holds, and shows nothing for one holding nothing', async () => {
    const deletedAt = '2026-11-02T12:00:00Z'
    const { unmount } = render(<div />)
    unmount()
    renderAttachments(makePage({ id: 'page', deletedAt, attachments: [ makeAttachment() ]}))

    await screen.findByRole('link', { name: 'Download plan.pdf' })

    expect(screen.queryByRole('button', { name: 'Attach files' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Remove plan.pdf' })).toBeNull()
  })
})
