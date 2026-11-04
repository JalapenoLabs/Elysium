// Copyright © 2026 Jalapeno Labs

import type { RunStep, StepAction } from '../../api/routes/runRoutes'

// Core
import { useTranslation } from 'react-i18next'
import { HTTPError } from 'ky'
import { mutate } from 'swr'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { runLoaded } from '../../store/runsSlice'

// Misc
import { actOnStep } from '../../api/routes/runRoutes'
import { nameFromIdentifier } from './runPresentation'
import { toast } from '../../toast/toast'

const doneKeys = {
  retry: 'step.done.retry',
  skip: 'step.done.skip',
  resume: 'step.done.resume',
} as const satisfies Record<StepAction, string>

// A person's retry, skip, or resume of a step. Each dispatches the run the API answers, so this
// tab updates before the event arrives, and reports its own failure.
export function useStepActions() {
  const { t } = useTranslation([ 'runs', 'common' ])
  const dispatch = useAppDispatch()

  return async function act(step: RunStep, action: StepAction) {
    const name = nameFromIdentifier(step.step)
    try {
      const response = await actOnStep(step.runId, step.id, action)
      dispatch(runLoaded(response))
      toast.success(t(doneKeys[action], { step: name }))
    }
    catch (error) {
      // The step moved on in the engine or another tab since this page drew it: show it as it is.
      if (error instanceof HTTPError && error.response.status === 409) {
        toast.danger(t('step.conflict'))
        void mutate(`v1/runs/${step.runId}`)
        return
      }
      console.debug(`useStepActions could not ${action} a step`, { error, stepId: step.id })
      toast.danger(t('common:errors.unexpected'))
    }
  }
}
