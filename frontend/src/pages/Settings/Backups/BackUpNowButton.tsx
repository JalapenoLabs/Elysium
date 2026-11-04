// Copyright © 2026 Jalapeno Labs

import type { BackupsStatus } from '../../../api/routes/backupRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Tooltip } from '@heroui/react'
import { LuDatabaseBackup } from 'react-icons/lu'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { startBackup } from '../../../api/routes/backupRoutes'
import { backUpNowBlockedKey } from './backupsPresentation'
import { toast } from '../../../toast/toast'

type Props = {
  status: BackupsStatus
}

// Backs up now. The API answers as soon as the backup begins; the event stream brings its
// progress into the history.
export function BackUpNowButton(props: Props) {
  const { t } = useTranslation([ 'backups', 'common' ])
  const [ isStarting, setIsStarting ] = useState(false)
  const blockedKey = backUpNowBlockedKey(props.status)

  async function backUpNow() {
    setIsStarting(true)
    try {
      await startBackup()
      toast.success(t('toasts.started'))
    }
    catch (error) {
      const message = getApiErrorMessage(error)
      if (!message) {
        console.debug('BackUpNowButton could not start a backup', { error })
      }
      toast.danger(t('toasts.failed'), {
        description: message ?? t('common:errors.unexpected'),
      })
    }
    setIsStarting(false)
  }

  const button = <div>
    <Button
      variant='outline'
      isDisabled={Boolean(blockedKey)}
      isPending={isStarting}
      onPress={backUpNow}
    >
      <LuDatabaseBackup className='size-4' aria-hidden />
      <span>{t('actions.backUpNow')}</span>
    </Button>
  </div>

  // A disabled button fires no hover events, so the tooltip's trigger is the wrapper, and it
  // wraps the button only while there is a reason to show.
  if (!blockedKey) {
    return button
  }
  return <Tooltip delay={200}>
    <Tooltip.Trigger>{
      button
    }</Tooltip.Trigger>
    <Tooltip.Content className='max-w-xs'>
      <span>{t(blockedKey)}</span>
    </Tooltip.Content>
  </Tooltip>
}
