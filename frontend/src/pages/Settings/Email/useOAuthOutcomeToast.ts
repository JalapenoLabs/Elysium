// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { useSearchParams } from 'react-router'

// User interface
import { toast } from '@heroui/react'

// Misc
import { oauthErrorMessageKeys } from './mailPresentation'

// The API's OAuth callback lands the browser back here with `mailConnected=<id>` or
// `mailError=<code>`. Announces the outcome once, then strips the parameters so a reload
// or a shared link does not announce it again.
export function useOAuthOutcomeToast() {
  const { t } = useTranslation('email')
  const [ searchParams, setSearchParams ] = useSearchParams()
  // StrictMode runs effects twice on mount in development; the ref keeps it to one toast.
  const hasAnnounced = useRef(false)

  const connectedId = searchParams.get('mailConnected')
  const errorCode = searchParams.get('mailError')

  useEffect(() => {
    if (hasAnnounced.current || (!connectedId && !errorCode)) {
      return
    }
    hasAnnounced.current = true

    if (connectedId) {
      toast.success(t('toasts.connected'))
    }
    else if (errorCode) {
      const messageKey = new Map(Object.entries(oauthErrorMessageKeys)).get(errorCode)
      if (!messageKey) {
        console.debug('OAuth callback returned an error code this build does not know', { errorCode })
      }
      toast.danger(t('toasts.connectFailed'), {
        description: t(messageKey ?? 'oauthErrors.provider_error'),
      })
    }

    setSearchParams({}, { replace: true })
  }, [ connectedId, errorCode, setSearchParams, t ])
}
