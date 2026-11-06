// Copyright © 2026 Jalapeno Labs

// Core
import { render, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// User interface
import { MarkdownContent } from './MarkdownContent'

// Misc
import { MARKED_PASSAGE_CLASS } from './rehypeMarkPassage'

const scrollIntoView = vi.fn()

beforeEach(() => {
  scrollIntoView.mockClear()
  Element.prototype.scrollIntoView = scrollIntoView
})

afterEach(() => {
  Reflect.deleteProperty(Element.prototype, 'scrollIntoView')
})

// The Markdown marked from the start of `from` to the end of `to`.
function renderMarked(markdown: string, from: string, to = from) {
  const markedRange = {
    start: markdown.indexOf(from),
    end: markdown.indexOf(to) + to.length,
  }
  return render(<MarkdownContent markdown={markdown} markedRange={markedRange} />)
}

function markedText(container: HTMLElement) {
  return Array.from(container.querySelectorAll(`.${MARKED_PASSAGE_CLASS}`), (block) => block.textContent?.trim())
}

describe('rehypeMarkPassage', () => {
  it('marks the blocks the range reaches and no others', () => {
    const { container } = renderMarked('First.\n\nSecond.\n\nThird.\n\nFourth.', 'Second.', 'Third.')

    expect(markedText(container)).toEqual([ 'Second.', 'Third.' ])
  })

  it('marks a heading however it renders', () => {
    const { container } = renderMarked('Before.\n\n# Title\n\nAfter.', '# Title')

    expect(container.querySelector(`h2.${MARKED_PASSAGE_CLASS}`)?.textContent).toBe('Title')
  })

  it('finds text whose Markdown is not what renders', () => {
    const { container } = renderMarked('Before.\n\nSee [the guide](https://example.com) **now**.', 'See [the guide]')

    expect(markedText(container)).toEqual([ 'See the guide now.' ])
  })

  it('marks a table through the component that draws it', () => {
    const { container } = renderMarked('Before.\n\n| Name |\n|:-----|\n| One  |\n\nAfter.', '| One')

    expect(container.querySelector(`.markdown-table.${MARKED_PASSAGE_CLASS}`)).not.toBeNull()
  })

  it('marks a code block through the component that draws it', () => {
    const { container } = renderMarked('Before.\n\n```rust\nfn main() {}\n```\n\nAfter.', 'fn main')

    expect(container.querySelector(`.markdown-code-block.${MARKED_PASSAGE_CLASS}`)).not.toBeNull()
  })

  it('marks the items a range reaches in a list it only partly covers', () => {
    const { container } = renderMarked('- One\n\n- Two\n\n- Three\n', 'Two')

    expect(markedText(container)).toEqual([ 'Two' ])
  })

  it('marks a whole list the range covers', () => {
    const { container } = renderMarked('Before.\n\n- One\n- Two\n\nAfter.', '- One', '- Two')

    expect(container.querySelector(`ul.${MARKED_PASSAGE_CLASS}`)).not.toBeNull()
  })

  it('brings the first marked block into view', async () => {
    const { container } = renderMarked('First.\n\nSecond.\n\nThird.', 'Second.', 'Third.')

    await waitFor(() => expect(scrollIntoView).toHaveBeenCalledOnce())
    expect(scrollIntoView.mock.contexts[0]).toBe(container.querySelector(`.${MARKED_PASSAGE_CLASS}`))
  })

  it('stays at the top when the range reaches nothing drawn', async () => {
    const markdown = 'Text.[^1]\n\n[^1]: A note.'
    const { container } = renderMarked(markdown, '[^1]: A note.')

    await new Promise((resolve) => {
      window.requestAnimationFrame(resolve)
    })
    expect(markedText(container)).toEqual([])
    expect(scrollIntoView).not.toHaveBeenCalled()
  })

  it('moves nobody when a range it already showed moves', async () => {
    const markdown = 'First.\n\nSecond.'
    const { rerender } = render(<MarkdownContent markdown={markdown} markedRange={{ start: 8, end: 15 }} />)
    await waitFor(() => expect(scrollIntoView).toHaveBeenCalledOnce())

    const edited = `Added.\n\n${markdown}`
    rerender(<MarkdownContent markdown={edited} markedRange={{ start: 16, end: 23 }} />)
    await new Promise((resolve) => {
      window.requestAnimationFrame(resolve)
    })

    expect(scrollIntoView).toHaveBeenCalledOnce()
  })
})
