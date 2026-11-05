// Copyright © 2026 Jalapeno Labs

import type { Editor } from '@tiptap/core'

// Core
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { act, render, screen, waitFor } from '@testing-library/react'

// User interface
import { MarkdownEditorContent } from './MarkdownEditorContent'

type Context = {
  onChange: ReturnType<typeof vi.fn<(markdown: string) => void>>
}

function renderEditor(markdown: string, onChange: (markdown: string) => void, isFocused = false) {
  const view = (current: string) => <MarkdownEditorContent
    id='body'
    label='Body'
    markdown={current}
    onChange={onChange}
    isFocused={isFocused}
  />

  const result = render(view(markdown))
  return {
    rerender: (next: string) => result.rerender(view(next)),
  } as const
}

// Tiptap hangs its editor on the text's element.
function getEditorView(textbox: HTMLElement) {
  return (textbox as HTMLElement & { editor: Editor }).editor.view
}

// The editor's contract with the form that owns its value: stored text loads untouched, the toolbar
// follows the selection, and only a person's edit reaches `onChange`.
describe('MarkdownEditorContent', () => {
  beforeEach<Context>((context) => {
    context.onChange = vi.fn<(markdown: string) => void>()
  })

  it<Context>('loads stored Markdown without reporting a change', async (context) => {
    renderEditor('* one\n* two', context.onChange)

    const textbox = await screen.findByRole('textbox', { name: 'Body' })

    expect(textbox.lastElementChild?.tagName).toBe('UL')
    expect(textbox.textContent).toBe('onetwo')
    expect(context.onChange).not.toHaveBeenCalled()
  })

  it<Context>('puts the caret in the text when asked', async (context) => {
    renderEditor('Text.', context.onChange, true)

    const textbox = await screen.findByRole('textbox', { name: 'Body' })

    await waitFor(() => {
      expect(document.activeElement).toBe(textbox)
    })
  })

  it<Context>('presses a toolbar button when the selection enters its block', async (context) => {
    renderEditor('# Heading', context.onChange)

    const heading = await screen.findByRole('button', { name: 'Heading 1' })

    await waitFor(() => {
      expect(heading.getAttribute('aria-pressed')).toBe('true')
    })
  })

  it<Context>('takes a value the form replaced without echoing it back', async (context) => {
    const { rerender } = renderEditor('draft', context.onChange)
    const textbox = await screen.findByRole('textbox', { name: 'Body' })

    rerender('restored')

    await waitFor(() => {
      expect(textbox.textContent).toBe('restored')
    })
    expect(context.onChange).not.toHaveBeenCalled()
  })

  it<Context>('reads pasted plain text as Markdown', async (context) => {
    // jsdom has no ClipboardEvent, and ProseMirror only hands it on.
    vi.stubGlobal('ClipboardEvent', Event)
    renderEditor('', context.onChange)
    const textbox = await screen.findByRole('textbox', { name: 'Body' })

    act(() => {
      getEditorView(textbox).pasteText('## Pasted\n\n- one\n- two')
    })

    await waitFor(() => {
      expect(context.onChange).toHaveBeenLastCalledWith('## Pasted\n\n- one\n- two')
    })
  })

  it<Context>('never writes an image address a browser would fetch, and reads its own HTML back', async (context) => {
    renderEditor('![A cat](https://example.com/cat.png)', context.onChange)
    const textbox = await screen.findByRole('textbox', { name: 'Body' })
    const { editor } = textbox as HTMLElement & { editor: Editor }

    const html = editor.getHTML()
    expect(html).not.toMatch(/\ssrc=/u)

    act(() => {
      editor.commands.setContent(html)
    })
    expect(editor.getJSON().content?.[0].content).toMatchObject([{ attrs: { src: 'https://example.com/cat.png' }}])
  })

  it<Context>('reports an edit as Markdown, leaving the rest of the text as it was', async (context) => {
    const { rerender } = renderEditor('* one\n\nA paragraph.', context.onChange)
    await screen.findByRole('textbox', { name: 'Body' })

    screen.getByRole('button', { name: 'Divider' }).click()

    await waitFor(() => {
      expect(context.onChange).toHaveBeenCalled()
    })
    const written = context.onChange.mock.lastCall?.[0] ?? ''
    expect(written).toContain('* one')
    expect(written).toContain('---')

    // Its own report coming back as the value changes nothing.
    rerender(written)
    expect(context.onChange).toHaveBeenCalledTimes(1)
  })
})
