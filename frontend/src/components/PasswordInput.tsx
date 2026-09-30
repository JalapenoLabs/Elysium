// Copyright © 2026 Jalapeno Labs

import type { PasswordAssessment } from '../hooks/usePasswordAssessment'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Description, FieldError, InputGroup, Label, TextField } from '@heroui/react'
import { LuCheck, LuEye, LuEyeOff, LuX } from 'react-icons/lu'

// Misc
import { PASSWORD_MIN_LENGTH } from '../constants'
import { PASSWORD_RULES } from './passwordRules'

type Props = {
  label: string
  value: string
  onChange: (value: string) => void
  // From `usePasswordAssessment`, which the form also reads to decide whether it may submit.
  assessment: PasswordAssessment
  // A refusal from Kratos, such as a breached password.
  errorMessage?: string
  isDisabled?: boolean
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

const ruleLabelKeys = {
  length: 'password.rules.length',
  uppercase: 'password.rules.uppercase',
  special: 'password.rules.special',
} as const

// Every field that sets a new password: a reveal toggle, a strength meter from zxcvbn, and
// the rules as a live checklist. The form stays disabled until the assessment accepts it.
export function PasswordInput(props: Props) {
  const { t } = useTranslation('auth')
  const [ isVisible, setIsVisible ] = useState(false)
  const score = props.assessment.strength?.score ?? null
  const hasValue = props.value.length > 0

  return <div>
    <TextField
      isRequired
      autoFocus={props.autoFocus}
      isDisabled={props.isDisabled}
      isInvalid={Boolean(props.errorMessage)}
      type={isVisible
        ? 'text'
        : 'password'}
      autoComplete='new-password'
      value={props.value}
      onChange={props.onChange}
    >
      <Label>{props.label}</Label>
      <InputGroup>
        <InputGroup.Input />
        <InputGroup.Suffix>
          <Button
            isIconOnly
            size='sm'
            variant='ghost'
            aria-label={isVisible
              ? t('password.hide')
              : t('password.show')}
            onPress={() => setIsVisible((visible) => !visible)}
          >
            {isVisible
              ? <LuEyeOff className='size-4' aria-hidden />
              : <LuEye className='size-4' aria-hidden />}
          </Button>
        </InputGroup.Suffix>
      </InputGroup>
      <FieldError>{props.errorMessage}</FieldError>
    </TextField>

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
      PASSWORD_RULES.map((rule) => {
        const isMet = props.assessment.rules[rule]
        return <li key={rule} className={isMet
          ? 'level-left gap-1.5 text-success'
          : 'level-left gap-1.5 opacity-70'}>
          {isMet
            ? <LuCheck className='size-3.5' aria-hidden />
            : <LuX className='size-3.5' aria-hidden />}
          <span>{t(ruleLabelKeys[rule], { count: PASSWORD_MIN_LENGTH })}</span>
        </li>
      })
    }</ul>
  </div>
}
