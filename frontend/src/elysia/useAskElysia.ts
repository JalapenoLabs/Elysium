// Copyright © 2026 Jalapeno Labs

// Core
import { useCallback } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../store/hooks'
import { elysiaMessageCreated, elysiaPanelToggled, elysiaPanelViewChanged } from '../store/elysiaSlice'

// Misc
import { sendElysiaMessage } from '../api/routes/elysiaRoutes'
import { toast } from '../toast/toast'

// Hands a question to Elysia from outside her panel, as search's "Ask Elysia about…" does:
// it opens her panel on the conversation and sends the question as the person's message,
// which queues her turn, so she starts on it without another step. Resolves whether it was
// sent; a failure says so in a toast, and the caller keeps the words so nothing is lost.
export function useAskElysia() {
  const { t } = useTranslation('elysia')
  const dispatch = useAppDispatch()

  return useCallback(async (question: string) => {
    const body = question.trim()
    if (!body) {
      console.debug('useAskElysia ignored an empty question')
      return false
    }

    dispatch(elysiaPanelToggled(true))
    dispatch(elysiaPanelViewChanged('conversation'))
    try {
      const response = await sendElysiaMessage(body)
      dispatch(elysiaMessageCreated(response.message))
      return true
    }
    catch (error) {
      console.debug('useAskElysia failed to send the question', { error })
      toast.danger(t('composer.failed'))
      return false
    }
  }, [ dispatch, t ])
}
