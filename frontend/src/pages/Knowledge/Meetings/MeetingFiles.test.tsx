// Copyright © 2026 Jalapeno Labs

import type { ComponentProps } from 'react'

// Core
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { Provider } from 'react-redux'
import { afterEach, describe, expect, it, vi } from 'vitest'

// Redux
import { createAppStore } from '../../../store'

// User interface
import { MeetingFiles } from './MeetingFiles'

// Misc
import { makeMeetingAttachment, makeMeetingAttachmentPolicy } from '../../../testFixtures'

type Props = ComponentProps<typeof MeetingFiles>

const roadmap = makeMeetingAttachment({ id: 'roadmap' })
const later = makeMeetingAttachment({
  id: 'later',
  kind: 'frame',
  fileName: 'Frame at 1m04s.webp',
  contentType: 'image/webp',
  displaysInline: true,
  caption: 'The launch checklist',
  captureId: 'capture',
  captureMs: 64_000,
  meetingMs: 64_000,
  textState: 'none',
})
const earlier = {
  ...later,
  id: 'earlier',
  fileName: 'Frame at 0m04s.webp',
  caption: 'The roadmap slide',
  captureMs: 4_000,
  meetingMs: 4_000,
}

function renderFiles(overrides: Partial<Props> = {}) {
  const props: Props = {
    attachments: [ roadmap, later, earlier ],
    hasFailed: false,
    policy: makeMeetingAttachmentPolicy(),
    attaching: [],
    onAttach: vi.fn(),
    onDetach: vi.fn(),
    onSelectFrame: vi.fn(),
    ...overrides,
  }
  render(<Provider store={createAppStore()}>
    <MeetingFiles {...props} />
  </Provider>)
  return props
}

describe('MeetingFiles', () => {
  afterEach(() => cleanup())

  it('lists frames in the order they were shown, then files, each saying how far its text got', () => {
    const props = renderFiles()

    const pictures = screen.getAllByRole('img').map((picture) => picture.getAttribute('alt'))
    expect(pictures).toEqual([ 'The roadmap slide', 'The launch checklist' ])
    expect(screen.getByRole('link', { name: 'Roadmap.pdf' }).getAttribute('href'))
      .toBe('/api/v1/meetings/meeting/attachments/roadmap/Roadmap.pdf')
    expect(screen.getByText(/Searchable/)).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: /The roadmap slide/ }))
    expect(props.onSelectFrame).toHaveBeenCalledWith(earlier)
  })

  it('attaches files chosen or dropped, and takes one off at once', () => {
    const props = renderFiles()
    const chosen = new File([ 'agenda' ], 'Agenda.docx')
    const dropped = new File([ 'notes' ], 'Notes.txt')

    const picker = document.querySelector('#meeting-attach-files')
    if (!(picker instanceof HTMLInputElement)) {
      throw new Error('no file picker')
    }
    fireEvent.change(picker, { target: { files: [ chosen ]}})
    const section = screen.getByRole('region', { name: 'Files and frames' })
    fireEvent.drop(section, { dataTransfer: { files: [ dropped ], types: [ 'Files' ]}})
    fireEvent.click(screen.getByRole('button', { name: 'Take Roadmap.pdf off the meeting' }))

    expect(props.onAttach).toHaveBeenNthCalledWith(1, [ chosen ])
    expect(props.onAttach).toHaveBeenNthCalledWith(2, [ dropped ])
    expect(props.onDetach).toHaveBeenCalledWith(roadmap)
  })

  it('says where files need to go before any can be attached, and takes none', () => {
    const props = renderFiles({ attachments: [], policy: makeMeetingAttachmentPolicy({ destinationId: null }) })

    expect(screen.getAllByText(/An admin chooses one in Settings, Storage, Attachments/).length)
      .toBeGreaterThan(0)
    expect(screen.getByRole('button', { name: 'Attach files' }).hasAttribute('disabled')).toBe(true)
    fireEvent.drop(screen.getByRole('region', { name: 'Files and frames' }), {
      dataTransfer: { files: [ new File([ 'x' ], 'x.txt') ], types: [ 'Files' ]},
    })
    expect(props.onAttach).not.toHaveBeenCalled()
  })

  it('offers nothing to change on a meeting that takes no change', () => {
    renderFiles({ onAttach: null, onDetach: null })

    expect(screen.queryByRole('button', { name: 'Attach files' })).toBeNull()
    expect(screen.queryByRole('button', { name: /off the meeting/ })).toBeNull()
    // What is attached still downloads.
    expect(screen.getByRole('link', { name: 'Roadmap.pdf' })).toBeTruthy()
  })

  it('shows each file on its way with how much of it arrived', () => {
    renderFiles({ attaching: [{ key: 'sending', name: 'Recording notes.pdf', progress: 0.4 }]})

    expect(screen.getByRole('progressbar', { name: 'Attaching Recording notes.pdf' }).getAttribute('aria-valuenow'))
      .toBe('40')
  })
})
