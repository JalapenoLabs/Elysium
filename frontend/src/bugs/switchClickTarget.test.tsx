// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'

// User interface
import { JiraScopeFields } from '../pages/Settings/Jira/JiraScopeFields'

// In HeroUI 3, `Switch.Content` is the clickable part of a switch (React Aria's SwitchButton),
// so it must wrap the control as well as the label. Every switch here had the control beside
// it instead: clicking the label toggled, clicking the toggle itself did nothing, and Settings,
// Users could not close sign-up from its toggle.

// Every source file, read as text, so the structure is checked wherever a switch is written.
const sources = import.meta.glob<string>(
  [ '../**/*.tsx', '!../**/*.test.tsx' ],
  { query: '?raw', import: 'default', eager: true },
)

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

describe('switches', () => {
  it('toggle when the control itself is clicked', () => {
    const onChange = vi.fn()
    const { container } = render(<JiraScopeFields
      projectOptions={[]}
      boardOptions={[]}
      projectsTruncated={false}
      boardsTruncated={false}
      value={{ allProjects: true, projectIds: [], allBoards: true, boardIds: []}}
      onChange={onChange}
      isProjectsDisabled={false}
      isBoardsDisabled={false}
    />)

    const controls = container.querySelectorAll('[data-slot="switch-control"]')
    expect(controls).toHaveLength(2)
    pressWithMouse(controls[0])

    expect(onChange).toHaveBeenCalledWith(expect.objectContaining({ allProjects: false }))
  })

  it('still toggle when the label is clicked', () => {
    const onChange = vi.fn()
    render(<JiraScopeFields
      projectOptions={[]}
      boardOptions={[]}
      projectsTruncated={false}
      boardsTruncated={false}
      value={{ allProjects: true, projectIds: [], allBoards: true, boardIds: []}}
      onChange={onChange}
      isProjectsDisabled={false}
      isBoardsDisabled={false}
    />)

    pressWithMouse(screen.getByText('All boards'))

    expect(onChange).toHaveBeenCalledWith(expect.objectContaining({ allBoards: false }))
  })

  it('keep every Switch.Control inside a Switch.Content', () => {
    const misplaced: string[] = []

    for (const [ path, source ] of Object.entries(sources)) {
      let index = source.indexOf('<Switch.Control>')
      while (index !== -1) {
        const before = source.slice(0, index)
        const isInsideContent = before.lastIndexOf('<Switch.Content') > before.lastIndexOf('</Switch.Content>')
        if (!isInsideContent) {
          misplaced.push(path)
        }
        index = source.indexOf('<Switch.Control>', index + 1)
      }
    }

    expect(misplaced).toEqual([])
  })
})
