// Copyright © 2026 Jalapeno Labs

import type { ProjectAutonomy, ProjectAutonomySetting } from '../../api/routes/projectRoutes'

// Core
import { fireEvent, render, screen } from '@testing-library/react'
import { Provider } from 'react-redux'
import { SWRConfig } from 'swr'
import { beforeEach, describe, expect, it, vi } from 'vitest'

// Redux
import { createAppStore } from '../../store'
import { usersLoaded } from '../../store/usersSlice'

// User interface
import { ProjectAutonomyField } from './ProjectAutonomyField'

// Misc
import { getProjectAutonomy } from '../../api/routes/projectRoutes'
import { formatDateTime } from '../../format/dates'
import { makeUser } from '../../testFixtures'

vi.mock('../../api/routes/projectRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../api/routes/projectRoutes')>(),
  getProjectAutonomy: vi.fn(),
}))

vi.mock('../../hooks/useServerData', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../hooks/useServerData')>(),
  useUsersLoader: () => 'loaded',
}))

const getAutonomy = vi.mocked(getProjectAutonomy)

function answer(setting: ProjectAutonomySetting) {
  getAutonomy.mockResolvedValue({ autonomy: setting })
}

function renderField(value: ProjectAutonomy, isEditable = true) {
  const onChange = vi.fn()
  const store = createAppStore()
  store.dispatch(usersLoaded([ makeUser({ id: 'ada', name: 'Ada Lovelace' }) ]))
  render(<SWRConfig value={{ provider: () => new Map() }}>
    <Provider store={store}>
      <ProjectAutonomyField
        projectId='launch'
        value={value}
        isEditable={isEditable}
        onChange={onChange}
      />
    </Provider>
  </SWRConfig>)
  return onChange
}

beforeEach(() => {
  vi.clearAllMocks()
})

describe('ProjectAutonomyField', () => {
  it('offers the three levels with the form value chosen', async () => {
    answer({ autonomy: 'approve_every_batch', setBy: null, setAt: null })
    renderField('approve_every_batch')

    const levels = screen.getAllByRole('radio')
    expect(levels.map((level) => level.getAttribute('value'))).toEqual([
      'approve_every_batch',
      'approve_found_work',
      'full_autonomy',
    ])
    expect(screen.getByRole('radio', { name: /Approve every batch/ })).toHaveProperty('checked', true)
    expect(await screen.findByText('Every project starts at Approve every batch')).toBeTruthy()
  })

  it('hands a chosen level to the form rather than saving it', () => {
    answer({ autonomy: 'approve_every_batch', setBy: null, setAt: null })
    const onChange = renderField('approve_every_batch')

    fireEvent.click(screen.getByRole('radio', { name: /Full autonomy/ }))

    expect(onChange).toHaveBeenCalledWith('full_autonomy')
  })

  it('says who set the saved level and when', async () => {
    const setAt = '2026-11-09T15:04:00Z'
    answer({ autonomy: 'approve_found_work', setBy: 'ada', setAt })
    renderField('approve_found_work')

    expect(await screen.findByText(`Set by Ada Lovelace on ${formatDateTime(setAt)}`)).toBeTruthy()
    expect(getAutonomy).toHaveBeenCalledWith('launch')
  })

  it('lets a guest read the level and who set it, but not turn it', async () => {
    answer({ autonomy: 'full_autonomy', setBy: 'ada', setAt: '2026-11-09T15:04:00Z' })
    const onChange = renderField('full_autonomy', false)

    for (const level of screen.getAllByRole('radio')) {
      expect(level).toHaveProperty('disabled', true)
    }
    fireEvent.click(screen.getByRole('radio', { name: /Approve every batch/ }))
    expect(onChange).not.toHaveBeenCalled()
    expect(screen.getByText('Only the company\'s members change this')).toBeTruthy()
    expect(await screen.findByText(/^Set by Ada Lovelace on /)).toBeTruthy()
  })
})
