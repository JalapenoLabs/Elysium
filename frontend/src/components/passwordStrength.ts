// Copyright © 2026 Jalapeno Labs

import type { ZxcvbnFactory } from '@zxcvbn-ts/core'

// How strong zxcvbn judges a password, 0 to 4, with its advice for a weaker one.
export type PasswordStrength = {
  score: number
  warning: string | null
  suggestions: string[]
}

let estimator: Promise<ZxcvbnFactory> | null = null

// zxcvbn's dictionaries are large, so they load on the first page that shows a password
// field, once, rather than with the app.
function loadEstimator() {
  estimator ??= Promise
    .all([
      import('@zxcvbn-ts/core'),
      import('@zxcvbn-ts/language-common'),
      import('@zxcvbn-ts/language-en'),
    ])
    .then(([ core, common, english ]) => new core.ZxcvbnFactory({
      dictionary: {
        ...common.dictionary,
        ...english.dictionary,
      },
      graphs: common.adjacencyGraphs,
      translations: english.translations,
    }))
  return estimator
}

// Judges `password`, treating `userInputs` (the person's name and email) as words anyone
// would guess first.
export async function estimatePasswordStrength(
  password: string,
  userInputs: string[],
): Promise<PasswordStrength> {
  const zxcvbn = await loadEstimator()
  const result = zxcvbn.check(password, userInputs.filter(Boolean))
  return {
    score: result.score,
    warning: result.feedback.warning || null,
    suggestions: result.feedback.suggestions,
  }
}
