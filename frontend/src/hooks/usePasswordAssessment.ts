// Copyright © 2026 Jalapeno Labs

import type { PasswordStrength } from '../components/passwordStrength'

// Core
import { useEffect, useState } from 'react'

// Misc
import { checkPasswordRules, isPasswordAcceptable } from '../components/passwordRules'
import { estimatePasswordStrength } from '../components/passwordStrength'

// Judges a new password as it is typed: the rules at once, zxcvbn's strength as soon as its
// dictionaries load. `isAcceptable` is what a form waits on before it submits.
export function usePasswordAssessment(password: string, userInputs: string[]) {
  const [ strength, setStrength ] = useState<{ password: string, result: PasswordStrength } | null>(null)
  const userInputsKey = userInputs.join('\n')

  useEffect(() => {
    let isCurrent = true
    estimatePasswordStrength(password, userInputsKey.split('\n'))
      .then((result) => {
        if (isCurrent) {
          setStrength({ password, result })
        }
      })
      .catch((error: unknown) => {
        console.debug('usePasswordAssessment could not judge a password', { error })
      })
    return () => {
      isCurrent = false
    }
  }, [ password, userInputsKey ])

  const rules = checkPasswordRules(password)
  // A result for an earlier value is stale the moment the value changes.
  const current = strength?.password === password
    ? strength.result
    : null

  return {
    rules,
    strength: current,
    isAcceptable: isPasswordAcceptable(rules, current?.score ?? null),
  } as const
}

export type PasswordAssessment = ReturnType<typeof usePasswordAssessment>
