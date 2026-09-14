// Copyright © 2026 Jalapeno Labs

import type { UseOverlayStateReturn } from '@heroui/react'
import type { Llm } from '../../../api/routes/llmRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { mutate } from 'swr'

// User interface
import { AlertDialog, Button, toast } from '@heroui/react'

// Misc
import { deleteLlm } from '../../../api/routes/llmRoutes'
import { LLMS_CACHE_KEY } from '../../../hooks/useLlms'

type Props = {
  state: UseOverlayStateReturn
  llm: Llm | null
}

export function DeleteLlmDialog(props: Props) {
  const { t } = useTranslation([ 'llms', 'common' ])
  const [ isDeleting, setIsDeleting ] = useState(false)

  async function onConfirm() {
    if (!props.llm) {
      console.debug('DeleteLlmDialog confirmed with no LLM selected')
      return
    }

    setIsDeleting(true)
    try {
      await deleteLlm(props.llm.id)
      await mutate(LLMS_CACHE_KEY)
      toast.success(t('toasts.deleted', { name: props.llm.name }))
      props.state.close()
    }
    catch (error) {
      console.debug('DeleteLlmDialog failed to delete the LLM', { error })
      toast.danger(t('common:errors.unexpected'))
    }
    finally {
      setIsDeleting(false)
    }
  }

  // Controlled overlays skip the AlertDialog root: it is a trigger wrapper, and without
  // a pressable child React Aria warns. The backdrop takes the open state directly.
  return <AlertDialog.Backdrop isOpen={props.state.isOpen} onOpenChange={props.state.setOpen}>
    <AlertDialog.Container>
      <AlertDialog.Dialog className='sm:max-w-md'>
        <AlertDialog.Header>
          <AlertDialog.Icon status='danger' />
          <AlertDialog.Heading>{
            t('delete.title', { name: props.llm?.name ?? '' })
          }</AlertDialog.Heading>
        </AlertDialog.Header>
        <AlertDialog.Body>
          <p>{
            t('delete.body')
          }</p>
        </AlertDialog.Body>
        <AlertDialog.Footer>
          <Button slot='close' variant='tertiary'>
            <span>{t('common:actions.cancel')}</span>
          </Button>
          <Button
            variant='danger'
            isPending={isDeleting}
            onPress={onConfirm}
          >
            <span>{t('common:actions.delete')}</span>
          </Button>
        </AlertDialog.Footer>
      </AlertDialog.Dialog>
    </AlertDialog.Container>
  </AlertDialog.Backdrop>
}
