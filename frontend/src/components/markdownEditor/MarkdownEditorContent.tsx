// Copyright © 2026 Jalapeno Labs

import type { AttachedFile } from './attachEditorFiles'
import type { EditorCommand } from './editorCommands'
import type { MarkdownSource } from './markdownSource'
import type { SlashMenuState } from './slashCommands'

// Core
import { Placeholder } from '@tiptap/extensions'
import { Slice } from '@tiptap/pm/model'
import { EditorContent, useEditor } from '@tiptap/react'
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { EditorImageDialog } from './EditorImageDialog'
import { EditorLinkDialog } from './EditorLinkDialog'
import { EditorToolbar } from './EditorToolbar'
import { SlashMenu } from './SlashMenu'

// Misc
import { toast } from '../../toast/toast'
import { attachEditorFiles } from './attachEditorFiles'
import { AttachmentPlaceholders } from './attachmentPlaceholders'
import { markdownSchemaExtensions } from './editorSchema'
import { readMarkdown, writeMarkdown } from './markdownSource'
import { SlashCommands } from './slashCommands'
import './markdownEditor.css'

// Whether this field can take files, and what it does with them. Files live outside the Markdown,
// so whoever owns the field says where they go; this only puts a reference to each into the text.
export type EditorAttachments =
  | {
    canAttach: true
    // What the file picker offers, as an `accept` attribute.
    accept: string
    // Null when it was refused, having said why.
    attach: (file: File) => Promise<AttachedFile | null>
  }
  // Said on the toolbar's button before anyone tries, and again to whoever drops a file anyway.
  | { canAttach: false, reason: string }

export type MarkdownEditorProps = {
  // Given to the text itself, so the field can be found as any other.
  id: string
  label: string
  placeholder?: string
  // The page's text. A value the editor did not write itself, such as a draft put back, replaces
  // what it holds.
  markdown: string
  onChange: (markdown: string) => void
  // Puts the caret in the text as it opens.
  isFocused?: boolean
  // Left out, the field takes no files.
  attachments?: EditorAttachments
}

type EditorDialog = 'link' | 'image' | null

// The Tiptap editor behind `MarkdownEditor`, on a page's Markdown. Kept apart from it, as `Markdown`
// keeps its content, so ProseMirror loads with the first editor shown rather than with the app.
// What it holds is a document, and what it reports is Markdown: `markdownSource.ts` carries every
// block a person did not touch through unchanged, and nothing is reported until they touch one.
export function MarkdownEditorContent(props: MarkdownEditorProps) {
  const { t } = useTranslation('common')
  const [ slash, setSlash ] = useState<SlashMenuState | null>(null)
  const [ dialog, setDialog ] = useState<EditorDialog>(null)
  const sourceRef = useRef<MarkdownSource | null>(null)
  // What the editor last reported, or was given, which tells a change of its own from a new value.
  const writtenRef = useRef<string | null>(null)
  const slashKeysRef = useRef<((event: KeyboardEvent) => boolean) | null>(null)
  // ProseMirror keeps the handlers it was made with, so they read whatever this holds now.
  const takeFilesRef = useRef<((files: File[], position: number) => void) | null>(null)

  const [ extensions ] = useState(() => [
    ...markdownSchemaExtensions,
    AttachmentPlaceholders,
    Placeholder.configure({ placeholder: props.placeholder }),
    SlashCommands.configure({
      onChange: setSlash,
      onKeyDown: (event) => slashKeysRef.current?.(event) ?? false,
    }),
  ])

  const editor = useEditor({
    extensions,
    // Made in an effect rather than while rendering. Tiptap destroys an editor no effect has claimed
    // within a tick, and React runs the effects of what a `lazy` load revealed later than that.
    immediatelyRender: false,
    // What shows the editor's state subscribes to it (`EditorToolbar`), so a keystroke draws no more.
    shouldRerenderOnTransaction: false,
    editorProps: {
      attributes: {
        'id': props.id,
        'role': 'textbox',
        'aria-multiline': 'true',
        'aria-label': props.label,
        'class': 'markdown markdown-editor-text',
      },
      // Plain text pasted here is read as Markdown, which is what agents and other tools hand
      // people. Pasted into code it never gets here, and stays what it says.
      clipboardTextParser: (text, _context, _isPlain, view) => Slice.maxOpen(
        readMarkdown(text, view.state.schema).document.content,
      ),
      // A file from a screenshot or a folder arrives alone. One beside text is what a spreadsheet or
      // word processor copies, as a picture of that text, and the text is what was meant.
      handlePaste: (view, event) => {
        const transfer = event.clipboardData
        if (!takeFilesRef.current || !transfer?.files.length || transfer.getData('text/plain')) {
          return false
        }
        takeFilesRef.current([ ...transfer.files ], view.state.selection.from)
        return true
      },
      handleDrop: (view, event, _slice, isMovedInside) => {
        const files = event.dataTransfer?.files
        if (!takeFilesRef.current || isMovedInside || !files?.length) {
          return false
        }
        const dropped = view.posAtCoords({ left: event.clientX, top: event.clientY })
        takeFilesRef.current([ ...files ], dropped?.pos ?? view.state.selection.from)
        return true
      },
    },
    onUpdate: ({ editor: updated }) => {
      if (!sourceRef.current) {
        console.debug('MarkdownEditorContent ignored an update before its text loaded')
        return
      }
      const written = writeMarkdown(sourceRef.current, updated.state.doc)
      if (written !== writtenRef.current) {
        writtenRef.current = written
        props.onChange(written)
      }
    },
  })

  // Comparing first is what stops the editor's own keystrokes coming back and moving the caret. A
  // layout effect, so the text is in place before the first paint.
  useLayoutEffect(() => {
    if (!editor || props.markdown === writtenRef.current) {
      return
    }
    const source = readMarkdown(props.markdown, editor.schema)
    sourceRef.current = source
    writtenRef.current = props.markdown

    // Loading is not an edit: nothing to undo back past, and nothing to report.
    const { state, view } = editor
    view.dispatch(state.tr
      .replaceWith(0, state.doc.content.size, source.document.content)
      .setMeta('addToHistory', false)
      .setMeta('preventUpdate', true))
  }, [ editor, props.markdown ])

  const { isFocused } = props
  useEffect(() => {
    if (isFocused) {
      editor?.commands.focus('start', { scrollIntoView: false })
    }
  }, [ editor, isFocused ])

  const { attachments } = props
  useEffect(() => {
    if (!editor || !attachments) {
      takeFilesRef.current = null
      return
    }
    takeFilesRef.current = (files, position) => {
      if (!attachments.canAttach) {
        console.debug('MarkdownEditorContent kept out a file it cannot take', { reason: attachments.reason })
        toast.warning(attachments.reason)
        return
      }
      void attachEditorFiles(editor, files, position, {
        attach: attachments.attach,
        uploadingLabel: (name) => t('markdownEditor.uploading', { name }),
      })
    }
  }, [ editor, attachments, t ])

  if (!editor) {
    return <div className='markdown-editor' />
  }

  function chooseSlashCommand(command: EditorCommand) {
    if (!editor || !slash) {
      console.debug('MarkdownEditorContent ignored a slash command with no menu open', { id: command.id })
      return
    }
    const chain = editor.chain().focus().deleteRange(slash.range)
    if (command.apply) {
      command.apply(chain).run()
      return
    }
    chain.run()
    setDialog('image')
  }

  return <div className='markdown-editor'>
    <EditorToolbar
      editor={editor}
      onOpenLink={() => setDialog('link')}
      onOpenImage={() => setDialog('image')}
      attachments={props.attachments}
      onChooseFiles={(files) => takeFilesRef.current?.(files, editor.state.selection.from)}
    />
    <EditorContent editor={editor} />

    {slash && <SlashMenu state={slash} keysRef={slashKeysRef} onChoose={chooseSlashCommand} />}

    {dialog === 'link' && <EditorLinkDialog
      address={editor.getAttributes('link').href ?? ''}
      onApply={(address) => {
        editor.chain().focus().extendMarkRange('link').setLink({ href: address }).run()
        setDialog(null)
      }}
      onRemove={() => {
        editor.chain().focus().extendMarkRange('link').unsetLink().run()
        setDialog(null)
      }}
      onClose={() => setDialog(null)}
    />}

    {dialog === 'image' && <EditorImageDialog
      onInsert={(address, description) => {
        editor.chain().focus().setImage({ src: address, alt: description }).run()
        setDialog(null)
      }}
      onClose={() => setDialog(null)}
    />}
  </div>
}
