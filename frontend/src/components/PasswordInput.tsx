// Copyright © 2026 Jalapeno Labs

import type { PasswordAssessment } from '../hooks/usePasswordAssessment'
import type { PasswordRequirement } from './passwordRules'

// Core
import { useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Description } from '@heroui/react'
import { LuCheck, LuX } from 'react-icons/lu'
import { SecretInput } from './SecretInput'

// Misc
import { PASSWORD_MIN_LENGTH } from '../constants'
import { PASSWORD_REQUIREMENTS } from './passwordRules'

type Props = {
  label: string
  value: string
  onChange: (value: string) => void
  // From `usePasswordAssessment`, which the form also reads to decide whether it may submit.
  assessment: PasswordAssessment
  // A refusal from Kratos, such as a breached password.
  errorMessage?: string
  isDisabled?: boolean
  // Focuses the field once it mounts, with a ref rather than the attribute, which is unreliable
  // in a modal that animates in.
  autoFocus?: boolean
}

// Segment colors by zxcvbn score, 0 (guessed at once) to 4 (strong).
const scoreColors = [ 'bg-danger', 'bg-danger', 'bg-warning', 'bg-success', 'bg-success' ] as const
const scoreLabelKeys = [
  'password.strength.veryWeak',
  'password.strength.weak',
  'password.strength.fair',
  'password.strength.strong',
  'password.strength.veryStrong',
] as const

const requirementLabelKeys = {
  length: 'password.rules.length',
  uppercase: 'password.rules.uppercase',
  special: 'password.rules.special',
  strength: 'password.rules.strength',
} as const satisfies Record<PasswordRequirement, string>

// Every field that sets a new password: a reveal toggle, a strength meter from zxcvbn, and every
// requirement as a live checklist, a check for each one met and a red X for each one not
// (`~/.claude/docs/ux.md`, Password inputs). The form stays disabled until all are met.
export function PasswordInput(props: Props) {
  const { t } = useTranslation('auth')
  const inputRef = useRef<HTMLInputElement>(null)
  const score = props.assessment.strength?.score ?? null
  const hasValue = props.value.length > 0
  const shouldFocus = props.autoFocus

  useEffect(() => {
    if (shouldFocus) {
      inputRef.current?.focus()
    }
  }, [ shouldFocus ])

  return <div>
    <SecretInput
      isRequired
      label={props.label}
      autoComplete='new-password'
      isDisabled={props.isDisabled}
      errorMessage={props.errorMessage}
      value={props.value}
      onChange={props.onChange}
      inputRef={inputRef}
    />

    {hasValue && <div className='mt-2'>
      <div className='level-left gap-1' aria-hidden>{
        [ 0, 1, 2, 3 ].map((segment) => <span
          key={segment}
          className={[
            'h-1 flex-1 rounded-full',
            score !== null && segment < Math.max(score, 1)
              ? scoreColors[score]
              : 'bg-default',
          ].join(' ')}
        />)
      }</div>
      <p className='mt-1 text-xs opacity-70' aria-live='polite'>{
        score === null
          ? t('password.strength.checking')
          : t('password.strength.label', { strength: t(scoreLabelKeys[score]) })
      }</p>
      {props.assessment.strength?.warning && <Description className='text-xs text-warning'>{
        props.assessment.strength.warning
      }</Description>}
    </div>}

    <ul className='mt-2 flex flex-col gap-1 text-xs'>{
      PASSWORD_REQUIREMENTS.map((requirement) => {
        const isMet = props.assessment.requirements[requirement]
        return <li key={requirement} className={isMet
          ? 'level-left gap-1.5 text-success'
          : 'level-left gap-1.5'}>
          {isMet
            ? <LuCheck className='size-3.5' aria-hidden />
            : <LuX className='size-3.5 text-danger' aria-hidden />}
          <span className={isMet
            ? undefined
            : 'opacity-70'}>{
              t(requirementLabelKeys[requirement], { count: PASSWORD_MIN_LENGTH })
            }</span>
        </li>
      })
    }</ul>
  </div>
}
