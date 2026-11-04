// Copyright © 2026 Jalapeno Labs

import type { UiText } from '@ory/client-fetch'

// Core
import { useCallback, useRef } from 'react'
import { useTranslation } from 'react-i18next'

// Utility
import { UiTextTypeEnum } from '@ory/client-fetch'

// Misc
import { describeKratosMessage } from '../pages/Auth/kratosPresentation'
import { toast } from '../toast/toast'

// How each kind of Kratos message shows. An error stays until dismissed.
const showByMessageType = {
  [UiTextTypeEnum.Error]: toast.danger,
  [UiTextTypeEnum.Info]: toast,
  [UiTextTypeEnum.Success]: toast.success,
  [UiTextTypeEnum.UnknownDefaultOpenApi]: toast,
} as const satisfies Record<UiTextTypeEnum, (message: string) => string>

// Shows the messages Kratos sent for a whole form, such as "wrong email or password", as
// toasts, so nothing they say moves the page; a field's own message stays on its field.
//
// Each call replaces the toasts the last one showed. Errors stay until dismissed, so without
// that, every retry of a refused form would stack another copy of the same refusal.
//
// Kratos's success, which it reports after every save, is dropped: the page's own toast says
// it in Elysium's words.
export function useKratosMessageToasts() {
  const { t } = useTranslation('auth')
  const shownIds = useRef<string[]>([])

  return useCallback((messages: UiText[] | undefined) => {
    for (const toastId of shownIds.current) {
      toast.close(toastId)
    }
    shownIds.current = []

    for (const message of messages ?? []) {
      if (message.type === UiTextTypeEnum.Success) {
        continue
      }
      const described = describeKratosMessage(message)
      const text = described.key
        ? t(described.key)
        : described.text
      shownIds.current.push(showByMessageType[message.type](text))
    }
  }, [ t ])
}
