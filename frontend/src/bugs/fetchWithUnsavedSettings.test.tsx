// Copyright © 2026 Jalapeno Labs

import type { WorkspaceSettings } from '../api/routes/userRoutes'

// Core
import { describe, expect, it } from 'vitest'
import { render, screen } from '@testing-library/react'

// User interface
import { AgentConfigurationStatus } from '../pages/Settings/Llms/AgentConfigurationStatus'

// On Agent configuration, a person picked a new GitHub token and pressed Fetch now without
// saving. A fetch reads the saved settings, so it kept using the old token and failed with a 403
// while the page showed the new one. Fetch now now waits for Save while the page has unsaved
// changes, and says so.

// The button itself: a disabled button sits in the tooltip's trigger, a focusable element of its
// own that takes the button's name too.
function fetchNowButton() {
  const button = screen.getByText('Fetch now').closest('button')
  if (!button) {
    throw new Error('Fetch now is not a button')
  }
  return button
}

const settings: WorkspaceSettings = {
  signupOpen: false,
  requireMfa: false,
  agentConfigurationRepository: 'https://github.com/someone/agents.git',
  agentConfigurationGithubCredentialId: null,
  agentConfiguration: { hold: null, checkout: null },
  workerPoolSize: null,
  updatedBy: 'admin',
  updatedAt: '2026-11-10T21:39:20Z',
}

describe('Fetch now on Agent configuration', () => {
  it('waits for Save while the page has unsaved changes', () => {
    render(<AgentConfigurationStatus settings={settings} hasUnsavedChanges />)

    expect(fetchNowButton().disabled).toBe(true)
  })

  it('fetches once everything on the page is saved', () => {
    render(<AgentConfigurationStatus settings={settings} hasUnsavedChanges={false} />)

    expect(fetchNowButton().disabled).toBe(false)
  })
})
