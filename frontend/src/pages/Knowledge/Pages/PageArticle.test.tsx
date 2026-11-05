// Copyright © 2026 Jalapeno Labs

// Core
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

// User interface
import { PageArticle } from './PageArticle'

vi.mock('../../../components/markdown/Markdown', () => ({
  Markdown: () => <div>The rendered text</div>,
}))

describe('PageArticle', () => {
  it('reads top to bottom', () => {
    const { container } = render(<PageArticle
      markdown='The text'
      byline={<span>Byline</span>}
      labels={<span>Labels</span>}
      attachments={<span>Attachments</span>}
      childPages={<span>Children</span>}
    />)

    expect(container.textContent).toBe('BylineLabelsThe rendered textAttachmentsChildren')
  })

  it('leaves no box for what a page does not have', () => {
    const { container } = render(<PageArticle markdown='The text' byline={<span>Byline</span>} />)

    expect(container.querySelector('article')?.children).toHaveLength(2)
  })

  it('goes straight to the children of a page with no text', () => {
    render(<PageArticle markdown='' byline={<span>Byline</span>} childPages={<span>Children</span>} />)

    expect(screen.queryByText('The rendered text')).toBeNull()
    expect(screen.queryByText('This page has no text yet.')).toBeNull()
    expect(screen.getByText('Children')).toBeDefined()
  })

  it('says a page with neither is empty', () => {
    render(<PageArticle markdown='' byline={<span>Byline</span>} />)

    expect(screen.getByText('This page has no text yet.')).toBeDefined()
  })
})
