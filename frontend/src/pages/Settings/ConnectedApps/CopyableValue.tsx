// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, toast } from '@heroui/react'
import { LuCopy } from 'react-icons/lu'

type Props = {
  label: string
  value: string
}

// One value to paste somewhere else, such as a command, with a button that copies it.
export function CopyableValue(props: Props) {
  const { t } = useTranslation('oauth')

  async function copy() {
    try {
      await navigator.clipboard.writeText(props.value)
      toast.success(t('connectedApps.connect.copied'))
    }
    catch (error) {
      console.debug('CopyableValue could not copy', { error, label: props.label })
      toast.danger(t('connectedApps.connect.copyFailed'))
    }
  }

  return <div className='compact'>
    <p className='mb-1 text-xs font-medium opacity-70'>{props.label}</p>
    <div className='level gap-2 rounded-lg bg-surface-secondary px-3 py-2'>
      <code className='min-w-0 flex-1 overflow-x-auto whitespace-nowrap text-sm'>{props.value}</code>
      <Button size='sm' variant='ghost' onPress={() => void copy()}>
        <LuCopy className='size-4' aria-hidden />
        <span>{t('connectedApps.connect.copy')}</span>
      </Button>
    </div>
  </div>
}
