// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { FieldError, Input, Label, TextField } from '@heroui/react'

// Misc
import { NAME_PART_MAX_LENGTH } from '../constants'

type Props = {
  firstName: string
  lastName: string
  onFirstNameChange: (value: string) => void
  onLastNameChange: (value: string) => void
  // Kratos's refusal of either, such as a name that is too long.
  firstNameError?: string
  lastNameError?: string
  autoFocus?: boolean
}

// First and last name side by side, first on the left, wherever a person's name is entered:
// sign-up and My profile. The last name is optional, for people with one name.
export function PersonNameFields(props: Props) {
  const { t } = useTranslation('auth')

  return <div className='grid grid-cols-1 gap-4 sm:grid-cols-2'>
    <TextField
      isRequired
      autoFocus={props.autoFocus}
      autoComplete='given-name'
      maxLength={NAME_PART_MAX_LENGTH}
      isInvalid={Boolean(props.firstNameError)}
      value={props.firstName}
      onChange={props.onFirstNameChange}
    >
      <Label>{t('fields.firstName')}</Label>
      <Input />
      <FieldError>{props.firstNameError}</FieldError>
    </TextField>
    <TextField
      autoComplete='family-name'
      maxLength={NAME_PART_MAX_LENGTH}
      isInvalid={Boolean(props.lastNameError)}
      value={props.lastName}
      onChange={props.onLastNameChange}
    >
      <Label>{t('fields.lastName')}</Label>
      <Input />
      <FieldError>{props.lastNameError}</FieldError>
    </TextField>
  </div>
}
