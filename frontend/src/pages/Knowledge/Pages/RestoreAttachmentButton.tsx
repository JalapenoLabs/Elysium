// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { pageLoaded } from '../../../store/pagesSlice'

// User interface
import { Button } from '@heroui/react'
import { LuHistory } from 'react-icons/lu'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { restorePageAttachment } from '../../../api/routes/pageRoutes'
import { toast } from '../../../toast/toast'

type Props = {
  pageId: string
  attachmentId: string
  name: string
}

// Puts a file back on its page, from the entry in its history that took it off.
export function RestoreAttachmentButton(props: Props) {
  const { t } = useTranslation('knowledge')
  const dispatch = useAppDispatch()
  const [ isRestoring, setIsRestoring ] = useState(false)
  const { pageId, attachmentId, name } = props

  async function restore() {
    setIsRestoring(true)
    try {
      const response = await restorePageAttachment(pageId, attachmentId)
      dispatch(pageLoaded(response.page))
      toast.success(t('attachments.restored', { name }))
    }
    catch (error) {
      console.debug('RestoreAttachmentButton failed', { error, pageId, attachmentId })
      toast.danger(t('attachments.restoreFailed', { name }), { description: getApiErrorMessage(error) })
    }
    finally {
      setIsRestoring(false)
    }
  }

  return <Button
    size='sm'
    variant='ghost'
    aria-label={t('attachments.restoreLabel', { name })}
    isPending={isRestoring}
    onPress={() => void restore()}
  >
    <LuHistory className='size-4' aria-hidden />
    <span>{t('attachments.restore')}</span>
  </Button>
}
