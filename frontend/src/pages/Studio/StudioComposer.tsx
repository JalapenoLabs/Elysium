// Copyright © 2026 Jalapeno Labs

import type { CodingSession } from '../../api/routes/codingSessionRoutes'
import type { StudioItem } from '../../api/routes/studioRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { codingSessionUpserted } from '../../store/codingSessionsSlice'
import { useAppDispatch } from '../../store/hooks'
import { studioFeedbackCreated } from '../../store/studioFeedbackSlice'

// User interface
import { toast } from '@heroui/react'
import { PromptComposer } from '../Coding/PromptComposer'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { sendStudioTurn } from '../../api/routes/studioRoutes'
import { useStudioItemLoader } from '../../hooks/useServerData'

type Props = {
  item: StudioItem
  // The item's newest session; absent until its sessions load, or when it has none.
  latestSession: CodingSession | undefined
}

// Sends the next prompt. It never closes: the item's session lives in the workspace, and an
// item whose session could not be opened gets one with the next prompt.
export function StudioComposer(props: Props) {
  const { t } = useTranslation([ 'studio', 'common' ])
  const dispatch = useAppDispatch()
  // The item page loads the item too; SWR shares the request. It says whether no session
  // means the item never ran or that its sessions have not arrived yet.
  const itemLoadStatus = useStudioItemLoader(props.item.id)

  async function send(prompt: string) {
    try {
      const response = await sendStudioTurn(props.item.id, { prompt })
      dispatch(codingSessionUpserted(response.session))
      if (response.feedback) {
        dispatch(studioFeedbackCreated(response.feedback))
      }
      return true
    }
    catch (error) {
      // A deleted item answers 409 and an unavailable workspace 502; both say what went wrong.
      const message = getApiErrorMessage(error)
      if (!message) {
        console.debug('StudioComposer failed to send a prompt', { error, itemId: props.item.id })
      }
      toast.danger(t('toasts.sendFailed'), {
        description: message ?? t('common:errors.unexpected'),
      })
      return false
    }
  }

  if (!props.latestSession && itemLoadStatus === 'loading') {
    return <PromptComposer
      onSend={send}
      sendBlocker={t('conversation.loading')}
    />
  }

  return <PromptComposer
    onSend={send}
    placeholder={props.latestSession
      ? t('conversation.placeholderContinue')
      : t('conversation.placeholderStart')}
  />
}
