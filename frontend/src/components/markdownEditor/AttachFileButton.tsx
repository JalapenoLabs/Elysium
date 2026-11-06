// Copyright © 2026 Jalapeno Labs

import type { EditorAttachments } from './MarkdownEditorContent'

// Core
import { useRef } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Tooltip } from '@heroui/react'
import { LuPaperclip } from 'react-icons/lu'

type Props = {
  attachments: EditorAttachments
  onChoose: (files: File[]) => void
}

// Picks files to attach at the caret. It stays in the toolbar when nothing can be attached, so the
// reason is one hover away before anyone goes looking for a file. A disabled button fires no hover
// of its own, so there the tooltip's trigger wraps it.
export function AttachFileButton(props: Props) {
  const { t } = useTranslation('common')
  const inputRef = useRef<HTMLInputElement>(null)
  const { attachments } = props

  if (!attachments.canAttach) {
    return <Tooltip delay={300}>
      <Tooltip.Trigger>
        <div>
          <Button isIconOnly size='sm' variant='ghost' aria-label={t('markdownEditor.attach')} isDisabled>
            <LuPaperclip className='size-4' aria-hidden />
          </Button>
        </div>
      </Tooltip.Trigger>
      <Tooltip.Content className='max-w-xs break-normal'>
        <span>{attachments.reason}</span>
      </Tooltip.Content>
    </Tooltip>
  }

  return <>
    <input
      ref={inputRef}
      type='file'
      multiple
      accept={attachments.accept}
      className='hidden'
      tabIndex={-1}
      aria-hidden
      onChange={(event) => {
        const files = [ ...event.currentTarget.files ?? [] ]
        // Clearing lets the same file be chosen again.
        event.currentTarget.value = ''
        props.onChoose(files)
      }}
    />
    <Tooltip delay={300}>
      <Button
        isIconOnly
        size='sm'
        variant='ghost'
        aria-label={t('markdownEditor.attach')}
        onPress={() => inputRef.current?.click()}
      >
        <LuPaperclip className='size-4' aria-hidden />
      </Button>
      <Tooltip.Content>
        <span>{t('markdownEditor.attach')}</span>
      </Tooltip.Content>
    </Tooltip>
  </>
}
