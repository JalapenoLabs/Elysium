// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'

// User interface
import { DeleteStudioItemMessage } from './DeleteStudioItemMessage'

const PERMANENT_LABEL = 'Also permanently delete its files, conversation, and agent memory'

describe('DeleteStudioItemMessage', () => {
  it('starts unchecked, so a delete is soft unless asked otherwise', () => {
    render(<DeleteStudioItemMessage onPermanentChange={vi.fn()} />)

    const checkbox = screen.getByRole('checkbox', { name: PERMANENT_LABEL })
    expect((checkbox as HTMLInputElement).checked).toBe(false)
    expect(screen.queryByText('This cannot be undone.')).toBeNull()
  })

  it('reports each change of the box and warns while it is checked', () => {
    const onPermanentChange = vi.fn()
    render(<DeleteStudioItemMessage onPermanentChange={onPermanentChange} />)
    const checkbox = screen.getByRole('checkbox', { name: PERMANENT_LABEL })

    fireEvent.click(checkbox)
    expect(onPermanentChange).toHaveBeenLastCalledWith(true)
    expect(screen.getByText('This cannot be undone.')).toBeTruthy()

    fireEvent.click(checkbox)
    expect(onPermanentChange).toHaveBeenLastCalledWith(false)
    expect(screen.queryByText('This cannot be undone.')).toBeNull()
  })
})
