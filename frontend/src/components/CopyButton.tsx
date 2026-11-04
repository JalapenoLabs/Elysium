// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Tooltip } from '@heroui/react'
import { LuCopy } from 'react-icons/lu'

// Misc
import { toast } from '../toast/toast'

type Props = {
  // What lands on the clipboard.
  value: string
  // Names what is copied, for the tooltip and screen readers, such as "Copy setup key".
  label: string
}

// An icon-only button that copies `value` and says so with a toast
// (`~/.claude/docs/ux.md`, Copyable inputs).
export function CopyButton(props: Props) {
  const { t } = useTranslation('common')

  async function copy() {
    try {
      await navigator.clipboard.writeText(props.value)
      toast.success(t('clipboard.copied'))
    }
    catch (error) {
      // Clipboard access needs a secure context and the page's focus; either can be missing.
      console.debug('CopyButton could not write to the clipboard', { error })
      toast.danger(t('clipboard.failed'))
    }
  }

  return <Tooltip delay={400}>
    <Button
      size='sm'
      variant='ghost'
      isIconOnly
      aria-label={props.label}
      onPress={() => void copy()}
    >
      <LuCopy className='size-4' aria-hidden />
    </Button>
    <Tooltip.Content>{props.label}</Tooltip.Content>
  </Tooltip>
}
