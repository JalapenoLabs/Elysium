// Copyright © 2026 Jalapeno Labs

import type { WorkerRepositoryValue } from './workerSetupFormSchema'

// Core
import { describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'

// User interface
import { WorkerRepositoryRow } from './WorkerRepositoryRow'

// React Aria handles presses through pointer events, so a bare click event is not a press.
// This is the sequence a mouse produces.
function pressWithMouse(element: Element) {
  const pointer = { pointerId: 1, pointerType: 'mouse', button: 0 }
  fireEvent.pointerDown(element, pointer)
  fireEvent.mouseDown(element)
  fireEvent.pointerUp(element, pointer)
  fireEvent.mouseUp(element)
  fireEvent.click(element)
}

function renderRow(value: Partial<WorkerRepositoryValue>, error?: string) {
  const onMergePolicyChange = vi.fn()
  render(<WorkerRepositoryRow
    index={0}
    count={1}
    value={{
      url: 'https://github.com/JalapenoLabs/web.git',
      baseBranch: '',
      setupScript: '',
      mergePolicy: 'person',
      ...value,
    }}
    errors={{ mergePolicy: error }}
    onChange={vi.fn()}
    onMergePolicyChange={onMergePolicyChange}
    onMove={vi.fn()}
    onRemove={vi.fn()}
  />)
  return onMergePolicyChange
}

describe('WorkerRepositoryRow', () => {
  it('leaves merging to a person until the switch says Elysium merges', () => {
    const onMergePolicyChange = renderRow({})
    const merges = screen.getByRole('switch', { name: 'Merge when ready' })
    expect(merges).toHaveProperty('checked', false)

    pressWithMouse(screen.getByText('Merge when ready'))
    expect(onMergePolicyChange).toHaveBeenCalledWith('auto')
  })

  it('hands a person merging back when the switch is turned off', () => {
    const onMergePolicyChange = renderRow({ mergePolicy: 'auto' })
    expect(screen.getByRole('switch', { name: 'Merge when ready' })).toHaveProperty('checked', true)

    pressWithMouse(screen.getByText('Merge when ready'))
    expect(onMergePolicyChange).toHaveBeenCalledWith('person')
  })

  it('says why Elysium cannot merge in a repository off github.com', () => {
    renderRow(
      { url: 'https://gitlab.com/team/api.git', mergePolicy: 'auto' },
      'Elysium merges pull requests on github.com repositories.',
    )
    expect(screen.getByText('Elysium merges pull requests on github.com repositories.')).toBeTruthy()
  })
})
