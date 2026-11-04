// Copyright © 2026 Jalapeno Labs

import type { ReactNode, Ref } from 'react'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Description, FieldError, InputGroup, Label, TextField, Tooltip } from '@heroui/react'
import { LuEye, LuEyeOff } from 'react-icons/lu'

type Props = {
  label: string
  value: string
  onChange: (value: string) => void
  // `off` for a secret pasted from elsewhere, such as a token, so the browser neither offers nor
  // saves it; `current-password` and `new-password` for the account's own password.
  autoComplete?: 'off' | 'current-password' | 'new-password'
  id: string
  name?: string
  placeholder?: string
  description?: ReactNode
  // Why the value is refused, shown under the field.
  errorMessage?: string | null
  isRequired?: boolean
  isInvalid?: boolean
  isDisabled?: boolean
  // Tokens and keys read better in a fixed-width face, where 0 and O differ.
  isMonospace?: boolean
  className?: string
  inputRef?: Ref<HTMLInputElement>
}

// Every secret field: hidden while typed, with an eye button that shows it and an eye with a
// slash that hides it again (`~/.claude/docs/ux.md`, Secret inputs). Passwords, tokens, API keys,
// and secret values all go through it, so each behaves the same.
export function SecretInput(props: Props) {
  const { t } = useTranslation('common')
  const [ isRevealed, setIsRevealed ] = useState(false)
  const {
    autoComplete = 'off',
  } = props

  const toggleLabel = isRevealed
    ? t('secret.hide')
    : t('secret.show')

  return <TextField
    id={props.id}
    name={props.name}
    className={props.className}
    isRequired={props.isRequired}
    isDisabled={props.isDisabled}
    isInvalid={props.isInvalid ?? Boolean(props.errorMessage)}
    type={isRevealed
      ? 'text'
      : 'password'}
    autoComplete={autoComplete}
    value={props.value}
    onChange={props.onChange}
  >
    <Label>{props.label}</Label>
    <InputGroup>
      <InputGroup.Input
        ref={props.inputRef}
        className={props.isMonospace
          ? 'font-mono'
          : undefined}
        spellCheck={false}
        placeholder={props.placeholder}
      />
      <InputGroup.Suffix>
        <Tooltip delay={400}>
          <Button
            isIconOnly
            size='sm'
            variant='ghost'
            aria-label={toggleLabel}
            aria-pressed={isRevealed}
            isDisabled={props.isDisabled}
            onPress={() => setIsRevealed((revealed) => !revealed)}
          >
            {isRevealed
              ? <LuEyeOff className='size-4' aria-hidden />
              : <LuEye className='size-4' aria-hidden />}
          </Button>
          <Tooltip.Content>{toggleLabel}</Tooltip.Content>
        </Tooltip>
      </InputGroup.Suffix>
    </InputGroup>
    {props.description && <Description>{props.description}</Description>}
    <FieldError>{props.errorMessage}</FieldError>
  </TextField>
}
