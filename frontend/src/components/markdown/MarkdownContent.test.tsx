// Copyright © 2026 Jalapeno Labs

// Core
import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

// User interface
import { MarkdownContent } from './MarkdownContent'

function renderMarkdown(markdown: string) {
  return render(<MarkdownContent markdown={markdown} />).container
}

describe('MarkdownContent', () => {
  it('renders tables, task lists, and strikethrough', () => {
    const container = renderMarkdown([
      '| Name | Count |',
      '|:-----|------:|',
      '| One  | 1     |',
      '',
      '- [x] Done',
      '',
      '~~gone~~',
    ].join('\n'))

    expect(container.querySelector('table th')?.textContent).toBe('Name')
    expect(container.querySelector('td:last-child')).toHaveProperty('style.textAlign', 'right')
    expect(container.querySelector('input[type="checkbox"]')).toHaveProperty('checked', true)
    expect(container.querySelector('del')?.textContent).toBe('gone')
  })

  it('marks a code block in a known language with class names, never inline styles', () => {
    const container = renderMarkdown('```rust\nfn main() {}\n```')

    expect(container.querySelector('code.language-rust .hljs-keyword')?.textContent).toBe('fn')
    expect(container.querySelector('pre [style]')).toBeNull()
  })

  it('leaves a block in a language it does not know plain, and readable', () => {
    const container = renderMarkdown('```not-a-language\nsome text\n```')

    expect(container.querySelector('pre code')?.textContent?.trim()).toBe('some text')
    expect(container.querySelector('pre span')).toBeNull()
  })

  it('leaves a page\'s title the one h1', () => {
    const container = renderMarkdown('# First\n\n## Second')

    expect(container.querySelector('h1')).toBeNull()
    expect(container.querySelector('h2')?.textContent).toBe('First')
    expect(container.querySelector('h3')?.textContent).toBe('Second')
  })
})

describe('MarkdownContent, given text meant to attack whoever reads it', () => {
  it('shows raw HTML as text and creates none of its elements', () => {
    const container = renderMarkdown([
      '<script>window.attacked = true</script>',
      '',
      '<img src="x" onerror="window.attacked = true">',
      '',
      '<iframe src="https://example.com"></iframe>',
      '',
      '<svg onload="window.attacked = true"></svg>',
      '',
      'A paragraph with <b onclick="window.attacked = true">inline HTML</b>.',
    ].join('\n'))

    for (const selector of [ 'script', 'img', 'iframe', 'svg', 'b', '[onclick]', '[onerror]', '[onload]' ]) {
      expect(container.querySelector(selector)).toBeNull()
    }
    expect(container.textContent).toContain('<script>window.attacked = true</script>')
  })

  it.each([
    '[click](javascript:alert(1))',
    '[click](JaVaScRiPt:alert(1))',
    '[click](java&#115;cript:alert(1))',
    '[click](&#106;avascript:alert(1))',
    '[click](vbscript:alert(1))',
    '[click](data:text/html;base64,PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==)',
  ])('keeps the words of %s and nothing to follow', (markdown) => {
    const container = renderMarkdown(markdown)

    expect(container.querySelector('a')).toBeNull()
    expect(container.textContent).toContain('click')
  })

  it.each([
    '![name](javascript:alert(1))',
    '![name](data:image/svg+xml;base64,PHN2Zz48L3N2Zz4=)',
  ])('loads no image from %s, and says so', (markdown) => {
    const container = renderMarkdown(markdown)

    expect(container.querySelector('img')).toBeNull()
    expect(container.textContent).toContain('name')
  })

  it('lets no attribute through on a link or an image', () => {
    const container = renderMarkdown('[a](/one "x\\" onclick=\\"y")\n\n![b](/two "x\\" onerror=\\"y")')

    expect(container.querySelector('[onclick]')).toBeNull()
    expect(container.querySelector('[onerror]')).toBeNull()
  })
})

describe('MarkdownContent links and images', () => {
  it('opens an external link in a new tab, without telling it where the reader came from', () => {
    const container = renderMarkdown('[Docs](https://example.com/docs)')
    const link = container.querySelector('a')

    expect(link?.getAttribute('href')).toBe('https://example.com/docs')
    expect(link?.getAttribute('target')).toBe('_blank')
    expect(link?.getAttribute('rel')).toBe('noreferrer')
  })

  it('keeps a link within Elysium in the same tab', () => {
    const link = renderMarkdown('[Ask](/knowledge/ask)').querySelector('a')

    expect(link?.getAttribute('href')).toBe('/knowledge/ask')
    expect(link?.hasAttribute('target')).toBe(false)
  })

  it('never loads an image from another origin, and links to it instead', () => {
    const container = renderMarkdown('![A cat](https://images.example.com/cat.png)')

    expect(container.querySelector('img')).toBeNull()
    expect(screen.getByRole('link').getAttribute('href')).toBe('https://images.example.com/cat.png')
    expect(container.textContent).toContain('A cat')
  })

  it('loads Elysium\'s own image, and says so when it breaks', () => {
    const container = renderMarkdown('![The diagram](/api/v1/attachments/1)')
    const image = container.querySelector('img')

    expect(image?.getAttribute('src')).toBe('/api/v1/attachments/1')
    expect(image?.getAttribute('alt')).toBe('The diagram')

    fireEvent.error(image as HTMLImageElement)

    expect(container.querySelector('img')).toBeNull()
    expect(container.textContent).toContain('The diagram')
  })
})
