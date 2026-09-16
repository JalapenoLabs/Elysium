// Copyright © 2026 Jalapeno Labs

import type { EnvironmentVariable } from '../../../api/routes/environmentRoutes'
import type { EnvironmentFormInput, EnvironmentFormValues } from './environmentFormSchema'

// Core
import { useMemo } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { environmentVariableUpserted } from '../../../store/environmentVariablesSlice'

// User interface
import {
  Button,
  Description,
  FieldError,
  Form,
  Input,
  Label,
  Switch,
  TextArea,
  TextField,
  toast,
} from '@heroui/react'
import { EnvironmentVisibilityNote } from './EnvironmentVisibilityNote'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'
import { HTTPError } from 'ky'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { createEnvironmentVariable, updateEnvironmentVariable } from '../../../api/routes/environmentRoutes'
import { createEnvironmentFormSchema } from './environmentFormSchema'

type Props = {
  // The variable being edited, or null to add a new one.
  variable: EnvironmentVariable | null
  onSaved: () => void
  onCancel: () => void
}

// Adds or edits an environment variable, beside a note on who can read it. A secret's value
// is write-only: when editing one, a blank field keeps the stored value, unless the variable
// is being made visible, which needs the value again.
export function EnvironmentVariableForm(props: Props) {
  const { t } = useTranslation([ 'environment', 'common' ])
  const dispatch = useAppDispatch()
  const stored = props.variable

  const resolver = useMemo(
    () => zodResolver(createEnvironmentFormSchema(t, stored)),
    [ t, stored ],
  )

  const form = useForm<EnvironmentFormInput, unknown, EnvironmentFormValues>({
    resolver,
    defaultValues: {
      key: stored?.key ?? '',
      // A secret's value never comes back, so its field starts blank.
      value: stored?.value ?? '',
      isSecret: stored?.isSecret ?? false,
      description: stored?.description ?? '',
    },
  })

  const onSubmit = form.handleSubmit(async (values) => {
    try {
      if (stored) {
        // A blank field keeps a secret's stored value, and an unchanged visible value needs
        // no re-sealing, so the value is only sent when there is a new one.
        const keepsValue = stored.isSecret
          ? !values.value
          : values.value === stored.value
        const response = await updateEnvironmentVariable(stored.id, {
          key: values.key,
          value: keepsValue
            ? undefined
            : values.value,
          isSecret: values.isSecret,
          description: values.description,
        })
        dispatch(environmentVariableUpserted(response.variable))
        toast.success(t('toasts.updated', { key: values.key }))
      }
      else {
        const response = await createEnvironmentVariable(values)
        dispatch(environmentVariableUpserted(response.variable))
        toast.success(t('toasts.created', { key: values.key }))
      }
      props.onSaved()
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 409) {
        form.setError('key', { message: t('form.errors.keyTaken') })
        return
      }

      // The form mirrors the API's rules, so a 400 here is one it did not foresee; its
      // message still names what to fix.
      const message = getApiErrorMessage(error)
      console.debug('EnvironmentVariableForm failed to save the variable', { error })
      toast.danger(t('common:errors.unexpected'), { description: message ?? undefined })
    }
  })

  const errors = form.formState.errors
  // useWatch subscribes per field and, unlike form.watch, is safe for the React Compiler.
  const values = useWatch({ control: form.control })
  const isSecret = values.isSecret ?? false
  const keepsStoredSecret = stored?.isSecret === true && isSecret
  const isRevealingSecret = stored?.isSecret === true && !isSecret

  function valueHint() {
    if (keepsStoredSecret) {
      return t('form.valueKeep')
    }
    if (isRevealingSecret) {
      return t('form.valueReveal')
    }
    return t('form.valueHint')
  }

  return <div className='grid grid-cols-1 items-start gap-8 lg:grid-cols-[minmax(0,1fr)_minmax(0,24rem)]'>
    <Form onSubmit={onSubmit} validationBehavior='aria' className='flex flex-col gap-4'>
      {/* Key */}
      <TextField
        autoFocus
        isRequired
        isInvalid={Boolean(errors.key)}
        value={values.key}
        onChange={(value) => form.setValue('key', value, { shouldDirty: true, shouldValidate: true })}
      >
        <Label>{t('form.key')}</Label>
        <Input className='font-mono' autoComplete='off' spellCheck={false} />
        <Description>{t('form.keyHint')}</Description>
        <FieldError>{errors.key?.message}</FieldError>
      </TextField>

      {/* Secret */}
      <Switch
        isSelected={isSecret}
        onChange={(isSelected) => form.setValue('isSecret', isSelected, { shouldDirty: true, shouldValidate: true })}
      >
        <Switch.Control>
          <Switch.Thumb />
        </Switch.Control>
        <Switch.Content>
          <Label>{t('form.isSecret')}</Label>
          <Description>{t('form.isSecretHint')}</Description>
        </Switch.Content>
      </Switch>

      {/* Value: a password field for a secret, so it is not shown while typed */}
      <TextField
        isRequired={isRevealingSecret || (isSecret && !keepsStoredSecret)}
        isInvalid={Boolean(errors.value)}
        value={values.value}
        type={isSecret
          ? 'password'
          : 'text'}
        onChange={(value) => form.setValue('value', value, { shouldDirty: true, shouldValidate: true })}
      >
        <Label>{t('form.value')}</Label>
        {isSecret
          ? <Input className='font-mono' autoComplete='off' />
          : <TextArea className='font-mono' rows={3} spellCheck={false} />}
        <Description>{valueHint()}</Description>
        <FieldError>{errors.value?.message}</FieldError>
      </TextField>

      {/* Description */}
      <TextField
        isInvalid={Boolean(errors.description)}
        value={values.description}
        onChange={(value) => form.setValue('description', value, { shouldDirty: true, shouldValidate: true })}
      >
        <Label>{t('form.description')}</Label>
        <TextArea rows={2} />
        <Description>{t('form.descriptionHint')}</Description>
        <FieldError>{errors.description?.message}</FieldError>
      </TextField>

      <div className='mt-2 flex justify-end gap-2'>
        <Button variant='tertiary' onPress={props.onCancel}>
          <span>{t('common:actions.cancel')}</span>
        </Button>
        <Button
          type='submit'
          isPending={form.formState.isSubmitting}
        >
          <span>{t('common:actions.save')}</span>
        </Button>
      </div>
    </Form>

    <aside className='lg:sticky lg:top-4'>
      <EnvironmentVisibilityNote />
    </aside>
  </div>
}
