// Copyright © 2026 Jalapeno Labs

// Core
import { render } from '@testing-library/react'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { describe, expect, it } from 'vitest'

// Redux
import { createAppStore } from '../store'

// User interface
import { MeetingLane } from '../pages/Knowledge/Meetings/MeetingLane'

// Misc
import { makeMeeting } from '../testFixtures'

// Found building #236: the options box at the top of a meeting's lane is `sticky`, but the lane was
// only as tall as its own content, so the box scrolled away with it as the transcript scrolled. A
// sticky box stays only inside its parent, so beside the transcript the lane must fill its grid
// cell, the transcript's whole height. jsdom lays nothing out, so the class that does it is checked.
function renderLane(isInDrawer: boolean) {
  const router = createMemoryRouter([{
    path: '*',
    element: <Provider store={createAppStore()}>
      <MeetingLane
        meeting={makeMeeting({ id: 'meeting' })}
        captures={[]}
        view='final'
        onViewChange={() => {}}
        shown={new Set()}
        onShownChange={() => {}}
        span={null}
        momentMs={null}
        words={[]}
        selectedMark={null}
        history={{ rows: [], isLoading: false, hasFailed: false }}
        onUndo={async () => {}}
        isInDrawer={isInDrawer}
      />
    </Provider>,
  }])
  const { container } = render(<RouterProvider router={router} />)
  const lane = container.firstElementChild
  if (!(lane instanceof HTMLElement)) {
    throw new Error('the lane rendered nothing')
  }
  return lane
}

describe('the options box beside a meeting\'s transcript', () => {
  it('sits in a lane as tall as the transcript, so it stays stuck as the page scrolls', () => {
    const lane = renderLane(false)

    expect(lane.classList.contains('h-full')).toBe(true)
    expect(lane.firstElementChild?.classList.contains('sticky')).toBe(true)
  })

  it('neither sticks nor fills in the phone\'s drawer, which scrolls the lane on its own', () => {
    const lane = renderLane(true)

    expect(lane.classList.contains('h-full')).toBe(false)
    expect(lane.firstElementChild?.classList.contains('sticky')).toBe(false)
  })
})
