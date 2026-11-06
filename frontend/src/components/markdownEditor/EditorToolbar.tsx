// Copyright © 2026 Jalapeno Labs

import type { Editor } from '@tiptap/core'
import type { IconType } from 'react-icons'
import type { EditorCommand } from './editorCommands'
import type { EditorAttachments } from './MarkdownEditorContent'

// Core
import { useEditorState } from '@tiptap/react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Tooltip } from '@heroui/react'
import { Toolbar } from 'react-aria-components'
import {
  LuBetweenHorizontalStart,
  LuBetweenVerticalStart,
  LuBold,
  LuCodeXml,
  LuItalic,
  LuLink,
  LuRedo2,
  LuStrikethrough,
  LuTableColumnsSplit,
  LuTableRowsSplit,
  LuTrash2,
  LuUndo2,
} from 'react-icons/lu'

// Misc
import { AttachFileButton } from './AttachFileButton'
import { editorCommands } from './editorCommands'

type Props = {
  editor: Editor
  onOpenLink: () => void
  onOpenImage: () => void
  attachments?: EditorAttachments
  onChooseFiles: (files: File[]) => void
}

type ToolbarButtonProps = {
  label: string
  icon: IconType
  isActive?: boolean
  onPress: () => void
}

const TOOLBAR_COMMANDS = new Set<EditorCommand['id']>([
  'heading1',
  'heading2',
  'heading3',
  'bulletList',
  'orderedList',
  'taskList',
  'quote',
  'codeBlock',
  'table',
  'divider',
  'image',
])

function ToolbarButton(props: ToolbarButtonProps) {
  const Icon = props.icon

  return <Tooltip delay={300}>
    <Button
      isIconOnly
      size='sm'
      variant={props.isActive
        ? 'secondary'
        : 'ghost'}
      aria-label={props.label}
      aria-pressed={props.isActive}
      onPress={props.onPress}
    >
      <Icon className='size-4' aria-hidden />
    </Button>
    <Tooltip.Content>
      <span>{props.label}</span>
    </Tooltip.Content>
  </Tooltip>
}

// The editor's formatting, kept in sight above the text as it scrolls. Tiptap 3 no longer draws the
// component holding the editor on every change, so pressed states subscribe here, compared by
// value, so a keystroke that changes none of them draws nothing.
export function EditorToolbar(props: Props) {
  const { t } = useTranslation('common')
  const { editor } = props

  const active = useEditorState({
    editor,
    selector: ({ editor: current }) => {
      const commands: Record<string, boolean> = {}
      for (const command of editorCommands) {
        commands[command.id] = Boolean(command.isActive?.(current))
      }
      return {
        commands,
        bold: current.isActive('bold'),
        italic: current.isActive('italic'),
        strike: current.isActive('strike'),
        code: current.isActive('code'),
        link: current.isActive('link'),
        table: current.isActive('table'),
      }
    },
  })

  return <Toolbar aria-label={t('markdownEditor.toolbarLabel')} className='markdown-editor-toolbar'>
    <div className='markdown-editor-toolbar-group'>
      <ToolbarButton
        label={t('markdownEditor.marks.bold')}
        icon={LuBold}
        isActive={active.bold}
        onPress={() => editor.chain().focus().toggleBold().run()}
      />
      <ToolbarButton
        label={t('markdownEditor.marks.italic')}
        icon={LuItalic}
        isActive={active.italic}
        onPress={() => editor.chain().focus().toggleItalic().run()}
      />
      <ToolbarButton
        label={t('markdownEditor.marks.strike')}
        icon={LuStrikethrough}
        isActive={active.strike}
        onPress={() => editor.chain().focus().toggleStrike().run()}
      />
      <ToolbarButton
        label={t('markdownEditor.marks.code')}
        icon={LuCodeXml}
        isActive={active.code}
        onPress={() => editor.chain().focus().toggleCode().run()}
      />
      <ToolbarButton
        label={t('markdownEditor.marks.link')}
        icon={LuLink}
        isActive={active.link}
        onPress={props.onOpenLink}
      />
    </div>

    <div className='markdown-editor-toolbar-group'>{
      editorCommands
        .filter((command) => TOOLBAR_COMMANDS.has(command.id))
        .map((command) => <ToolbarButton
          key={command.id}
          label={t(`markdownEditor.commands.${command.id}`)}
          icon={command.icon}
          isActive={active.commands[command.id]}
          onPress={() => {
            if (!command.apply) {
              props.onOpenImage()
              return
            }
            command.apply(editor.chain().focus()).run()
          }}
        />)
    }
    {props.attachments && <AttachFileButton
      attachments={props.attachments}
      onChoose={props.onChooseFiles}
    />}
    </div>

    <div className='markdown-editor-toolbar-group'>
      <ToolbarButton
        label={t('markdownEditor.undo')}
        icon={LuUndo2}
        onPress={() => editor.chain().focus().undo().run()}
      />
      <ToolbarButton
        label={t('markdownEditor.redo')}
        icon={LuRedo2}
        onPress={() => editor.chain().focus().redo().run()}
      />
    </div>

    {active.table && <div className='markdown-editor-toolbar-group'>
      <ToolbarButton
        label={t('markdownEditor.table.addRow')}
        icon={LuBetweenHorizontalStart}
        onPress={() => editor.chain().focus().addRowAfter().run()}
      />
      <ToolbarButton
        label={t('markdownEditor.table.addColumn')}
        icon={LuBetweenVerticalStart}
        onPress={() => editor.chain().focus().addColumnAfter().run()}
      />
      <ToolbarButton
        label={t('markdownEditor.table.deleteRow')}
        icon={LuTableRowsSplit}
        onPress={() => editor.chain().focus().deleteRow().run()}
      />
      <ToolbarButton
        label={t('markdownEditor.table.deleteColumn')}
        icon={LuTableColumnsSplit}
        onPress={() => editor.chain().focus().deleteColumn().run()}
      />
      <ToolbarButton
        label={t('markdownEditor.table.delete')}
        icon={LuTrash2}
        onPress={() => editor.chain().focus().deleteTable().run()}
      />
    </div>}
  </Toolbar>
}
