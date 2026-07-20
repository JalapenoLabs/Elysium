// Copyright © 2026 Jalapeno Labs

import type { StreamingSource } from './StreamingSources'

// Core
import { afterEach, describe, expect, it } from 'vitest'
import { cleanup, render, screen } from '@testing-library/react'

// Misc
import { StreamingSources } from './StreamingSources'

afterEach(() => {
  cleanup()
})

const sources: StreamingSource[] = [
  { id: 'local', label: 'Local Drive', status: 'done', detail: '12 results' },
  { id: 's3', label: 'S3 Archive', status: 'active' },
  { id: 'sftp', label: 'SFTP Vault', status: 'error', detail: 'Connection refused' },
]

describe('StreamingSources', () => {
  it('renders every source with its label and detail', () => {
    render(<StreamingSources sources={sources} />)
    expect(screen.getByText('Local Drive')).toBeTruthy()
    expect(screen.getByText('12 results')).toBeTruthy()
    expect(screen.getByText('S3 Archive')).toBeTruthy()
    expect(screen.getByText('SFTP Vault')).toBeTruthy()
    expect(screen.getByText('Connection refused')).toBeTruthy()
  })

  it('renders nothing for an empty source list', () => {
    const { container } = render(<StreamingSources sources={[]} />)
    expect(container.innerHTML).toBe('')
  })
})
