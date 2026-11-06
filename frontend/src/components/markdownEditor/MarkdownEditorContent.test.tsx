// Copyright © 2026 Jalapeno Labs

import type { Editor } from '@tiptap/core'
import type { EditorView } from '@tiptap/pm/view'
import type { AttachedFile } from './attachEditorFiles'
import type { EditorAttachments } from './MarkdownEditorContent'

// Core
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { Slice } from '@tiptap/pm/model'
import { TextSelection } from '@tiptap/pm/state'
import { act, render, screen, waitFor } from '@testing-library/react'

// User interface
import { MarkdownEditorContent } from './MarkdownEditorContent'

// Misc
import { toast } from '../../toast/toast'

vi.mock('../../toast/toast', () => ({
  toast: { warning: vi.fn() },
}))

type Context = {
  onChange: ReturnType<typeof vi.fn<(markdown: string) => void>>
}

function renderEditor(
  markdown: string,
  onChange: (markdown: string) => void,
  isFocused = false,
  attachments?: EditorAttachments,
) {
  const view = (current: string) => <MarkdownEditorContent
    id='body'
    label='Body'
    markdown={current}
    onChange={onChange}
    isFocused={isFocused}
    attachments={attachments}
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

function caretAt(view: EditorView, position: number) {
  view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, position)))
}

function pasteFiles(view: EditorView, files: File[], text = '') {
  const event = new Event('paste') as unknown as ClipboardEvent
  Object.defineProperty(event, 'clipboardData', {
    value: {
      files,
      getData: (type: string) => type === 'text/plain'
        ? text
        : '',
    },
  })
  return view.someProp('handlePaste', (handler) => handler(view, event, Slice.empty))
}

function dropFiles(view: EditorView, files: File[], position: number) {
  view.posAtCoords = () => ({ pos: position, inside: -1 })
  const event = new Event('drop') as unknown as DragEvent
  Object.defineProperty(event, 'dataTransfer', { value: { files }})
  return view.someProp('handleDrop', (handler) => handler(view, event, Slice.empty, false))
}

const PNG_URL = '/api/v1/pages/one/attachments/two/plan.png'
const PDF_URL = '/api/v1/pages/one/attachments/three/report.pdf'

// Files live outside the Markdown, and what they leave in it is plain Markdown, in the place they
// were put, with every other byte of the page as it was.
describe('MarkdownEditorContent attaching files', () => {
  beforeEach<Context>((context) => {
    context.onChange = vi.fn<(markdown: string) => void>()
    vi.mocked(toast.warning).mockReset()
  })

  function attaching(files: Record<string, AttachedFile | null>): EditorAttachments {
    return {
      canAttach: true,
      accept: '.png,.pdf',
      attach: async (file) => files[file.name] ?? null,
    }
  }

  const png = { name: 'plan.png', url: PNG_URL, isImage: true }
  const pdf = { name: 'report.pdf', url: PDF_URL, isImage: false }

  it<Context>('puts a pasted image where the caret is, leaving the rest exactly as it was', async (context) => {
    const body = '* kept\n* as  written\n\nA paragraph.'
    renderEditor(body, context.onChange, false, attaching({ 'plan.png': png }))
    const view = getEditorView(await screen.findByRole('textbox', { name: 'Body' }))
    const end = view.state.doc.content.size - 1
    caretAt(view, end)

    let isTaken: unknown = null
    act(() => {
      isTaken = pasteFiles(view, [ new File([ 'x' ], 'plan.png') ])
    })

    expect(isTaken).toBe(true)
    await waitFor(() => {
      expect(context.onChange).toHaveBeenLastCalledWith(`* kept\n* as  written\n\nA paragraph.![plan](${PNG_URL})`)
    })
  })

  it<Context>('puts a dropped file where it was dropped, as a link', async (context) => {
    renderEditor('Before after.', context.onChange, false, attaching({ 'report.pdf': pdf }))
    const view = getEditorView(await screen.findByRole('textbox', { name: 'Body' }))

    act(() => {
      dropFiles(view, [ new File([ 'x' ], 'report.pdf') ], 8)
    })

    await waitFor(() => {
      expect(context.onChange).toHaveBeenLastCalledWith(`Before [report.pdf](${PDF_URL})after.`)
    })
  })

  it<Context>('keeps files in order, apart, as they arrive one after another', async (context) => {
    renderEditor('', context.onChange, false, attaching({ 'report.pdf': pdf, 'plan.png': png }))
    const view = getEditorView(await screen.findByRole('textbox', { name: 'Body' }))

    act(() => {
      pasteFiles(view, [ new File([ 'x' ], 'report.pdf'), new File([ 'x' ], 'plan.png') ])
    })

    await waitFor(() => {
      expect(context.onChange).toHaveBeenLastCalledWith(`[report.pdf](${PDF_URL}) ![plan](${PNG_URL})`)
    })
  })

  it<Context>('follows the text typed while a file is on its way', async (context) => {
    let arrive: (file: AttachedFile) => void = () => undefined
    renderEditor('Text.', context.onChange, false, {
      canAttach: true,
      accept: '.png',
      attach: () => new Promise((resolve) => {
        arrive = resolve
      }),
    })
    const view = getEditorView(await screen.findByRole('textbox', { name: 'Body' }))

    act(() => {
      caretAt(view, 6)
      pasteFiles(view, [ new File([ 'x' ], 'plan.png') ])
    })
    expect(view.dom.querySelector('.markdown-editor-uploading')?.textContent).toBe('Uploading plan.png…')
    act(() => {
      view.dispatch(view.state.tr.insertText('New. ', 1))
    })
    await act(async () => {
      arrive(png)
    })

    await waitFor(() => {
      expect(context.onChange).toHaveBeenLastCalledWith(`New. Text.![plan](${PNG_URL})`)
    })
    expect(view.dom.querySelector('.markdown-editor-uploading')).toBeNull()
  })

  it<Context>('adds nothing for a file that was refused, and takes away its placeholder', async (context) => {
    renderEditor('Text.', context.onChange, false, attaching({}))
    const view = getEditorView(await screen.findByRole('textbox', { name: 'Body' }))

    act(() => {
      pasteFiles(view, [ new File([ 'x' ], 'tool.exe') ])
    })

    await waitFor(() => {
      expect(view.dom.querySelector('.markdown-editor-uploading')).toBeNull()
    })
    expect(context.onChange).not.toHaveBeenCalled()
  })

  it<Context>('sets a file after a code block rather than inside it', async (context) => {
    renderEditor('```\ncode\n```', context.onChange, false, attaching({ 'plan.png': png }))
    const view = getEditorView(await screen.findByRole('textbox', { name: 'Body' }))

    act(() => {
      caretAt(view, 3)
      pasteFiles(view, [ new File([ 'x' ], 'plan.png') ])
    })

    await waitFor(() => {
      expect(context.onChange).toHaveBeenLastCalledWith(`\`\`\`\ncode\n\`\`\`\n\n![plan](${PNG_URL})`)
    })
  })

  it<Context>('reads text copied beside a picture of it as text', async (context) => {
    vi.stubGlobal('ClipboardEvent', Event)
    renderEditor('', context.onChange, false, attaching({ 'plan.png': png }))
    const view = getEditorView(await screen.findByRole('textbox', { name: 'Body' }))

    let isTaken: unknown = null
    act(() => {
      isTaken = pasteFiles(view, [ new File([ 'x' ], 'plan.png') ], 'cell one')
    })

    expect(isTaken).toBeFalsy()
  })

  it<Context>('says why it cannot take a file, and has the toolbar say so first', async (context) => {
    renderEditor('Text.', context.onChange, false, { canAttach: false, reason: 'Publish first.' })
    const view = getEditorView(await screen.findByRole('textbox', { name: 'Body' }))
    const button = screen.getByRole('button', { name: 'Attach a file' })

    expect(button.hasAttribute('disabled') || button.getAttribute('aria-disabled') === 'true').toBe(true)

    let isTaken: unknown = null
    act(() => {
      isTaken = pasteFiles(view, [ new File([ 'x' ], 'plan.png') ])
    })

    expect(isTaken).toBe(true)
    expect(toast.warning).toHaveBeenCalledWith('Publish first.')
    expect(context.onChange).not.toHaveBeenCalled()
  })

  it<Context>('leaves files alone in a field that takes none', async (context) => {
    renderEditor('Text.', context.onChange)
    const view = getEditorView(await screen.findByRole('textbox', { name: 'Body' }))

    expect(screen.queryByRole('button', { name: 'Attach a file' })).toBeNull()
    expect(pasteFiles(view, [ new File([ 'x' ], 'plan.png') ])).toBeFalsy()
  })
})

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
