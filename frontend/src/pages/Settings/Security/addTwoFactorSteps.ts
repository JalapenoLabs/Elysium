// Copyright © 2026 Jalapeno Labs

// Misc
import { UrlTree } from '../../../urls'

// Adding two-factor authentication, in order: name the app, save its backup codes, then scan
// and verify the app.
export const ADD_TWO_FACTOR_STEPS = [ 'name', 'codes', 'app' ] as const
export type AddTwoFactorStep = typeof ADD_TWO_FACTOR_STEPS[number]

// Where the page keeps its progress in the address, so a reload resumes it.
export const ADD_TWO_FACTOR_PARAMS = {
  step: 'step',
  // What the person calls the app, once they moved past naming it.
  name: 'name',
  // The Kratos settings flows holding the unsaved backup codes and the app being verified.
  codesFlow: 'codesFlow',
  appFlow: 'appFlow',
} as const

// The step the address is on. A later step needs the app's name, so without one it is the
// first step, whatever the address says.
export function readAddTwoFactorStep(params: URLSearchParams): AddTwoFactorStep {
  const step = ADD_TWO_FACTOR_STEPS.find((candidate) => candidate === params.get(ADD_TWO_FACTOR_PARAMS.step))
  if (!step || !params.get(ADD_TWO_FACTOR_PARAMS.name)?.trim()) {
    return 'name'
  }
  return step
}

// The address of `step`, keeping the flows the page already holds. `name` replaces the app's
// name when given.
export function getAddTwoFactorStepUrl(params: URLSearchParams, step: AddTwoFactorStep, name?: string) {
  const next = new URLSearchParams(params)
  next.set(ADD_TWO_FACTOR_PARAMS.step, step)
  if (name !== undefined) {
    next.set(ADD_TWO_FACTOR_PARAMS.name, name)
  }
  return `${UrlTree.settingsTwoFactorNew}?${next.toString()}`
}
