// Copyright © 2026 Jalapeno Labs

import type { KeyValueEntry } from './KeyValueEditor'

// Core
import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'

// Misc
import { KeyValueEditor } from './KeyValueEditor'

// react-i18next needs no live backend here; the default t() returns the key,
// which is enough to find the controls.
vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

afterEach(() => {
  cleanup()
})

const entries: KeyValueEntry[] = [
  { key: 'region', value: 'us-east-1' },
  { key: 'bucket', value: 'elysium-drive' },
]

describe('KeyValueEditor', () => {
  it('renders one row per entry', () => {
    render(<KeyValueEditor entries={entries} onChange={() => {}} />)
    expect(screen.getByDisplayValue('region')).toBeTruthy()
    expect(screen.getByDisplayValue('us-east-1')).toBeTruthy()
    expect(screen.getByDisplayValue('bucket')).toBeTruthy()
  })

  it('appends a blank row on add', () => {
    const onChange = vi.fn()
    render(<KeyValueEditor entries={entries} onChange={onChange} />)

    fireEvent.click(screen.getByText('editor.addRow'))
    expect(onChange).toHaveBeenCalledWith([ ...entries, { key: '', value: '' }])
  })

  it('removes exactly the clicked row', () => {
    const onChange = vi.fn()
    render(<KeyValueEditor entries={entries} onChange={onChange} />)

    fireEvent.click(screen.getAllByLabelText('editor.removeRow')[0])
    expect(onChange).toHaveBeenCalledWith([ entries[1] ])
  })

  it('patches only the edited field of the edited row', () => {
    const onChange = vi.fn()
    render(<KeyValueEditor entries={entries} onChange={onChange} />)

    fireEvent.change(screen.getByDisplayValue('region'), { target: { value: 'endpoint' }})
    expect(onChange).toHaveBeenCalledWith([
      { key: 'endpoint', value: 'us-east-1' },
      entries[1],
    ])
  })

  it('masks values behind password inputs in secret mode', () => {
    render(<KeyValueEditor entries={entries} onChange={() => {}} isSecretValues />)
    const secretField = screen.getByDisplayValue('us-east-1')
    expect(secretField.getAttribute('type')).toBe('password')
  })
})
