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
  Description,
  FieldError,
  Form,
  Input,
  Label,
  Switch,
  TextArea,
  TextField,
} from '@heroui/react'
import { PageCreateFooter } from '../../../layout/page/PageCreateFooter'
import { SecretInput } from '../../../components/SecretInput'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'
import { HTTPError } from 'ky'

// Misc
import { useFormDraft } from '../../../drafts/useFormDraft'
import { getApiErrorMessage } from '../../../api/errors'
import { createEnvironmentVariable, updateEnvironmentVariable } from '../../../api/routes/environmentRoutes'
import { useInitialFocus } from '../../../hooks/useInitialFocus'
import { saveForm, usePageSave } from '../../../layout/page/pageSave'
import { createEnvironmentFormSchema } from './environmentFormSchema'
import { toast } from '../../../toast/toast'

type Props = {
  // The variable being edited, or null to add a new one.
  variable: EnvironmentVariable | null
  onSaved: () => void
}

// Adds or edits an environment variable, beside a note on who can read it. A secret's value
// is write-only: when editing one, a blank field keeps the stored value, unless the variable
// is being made visible, which needs the value again. Saved from the page's Save, in the header,
// which a new variable also has below the form as Create.
export function EnvironmentVariableForm(props: Props) {
  const { t } = useTranslation([ 'environment', 'common' ])
  const dispatch = useAppDispatch()
  const stored = props.variable

  const resolver = useMemo(
    () => zodResolver(createEnvironmentFormSchema(t, stored)),
    [ t, stored ],
  )

  const keyRef = useInitialFocus()
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
  // A new variable is kept for the tab until it is added; its value only while it is not secret.
  const forgetDraft = useFormDraft(form, {
    name: 'environment-variable',
    fields: (current) => current.isSecret
      ? [ 'key', 'description' ]
      : [ 'key', 'description', 'value' ],
    choices: [ 'isSecret' ],
    isEnabled: !stored,
  })

  const save = saveForm(form, async (values) => {
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
        forgetDraft()
        toast.success(t('toasts.created', { key: values.key }))
      }
      props.onSaved()
      return true
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 409) {
        form.setError('key', { message: t('form.errors.keyTaken') })
        return false
      }

      // The form mirrors the API's rules, so a 400 here is one it did not foresee; its
      // message still names what to fix.
      const message = getApiErrorMessage(error)
      console.debug('EnvironmentVariableForm failed to save the variable', { error })
      toast.danger(t('common:errors.unexpected'), { description: message ?? undefined })
      return false
    }
  })

  const onSubmit = usePageSave({
    save,
    onDiscard: forgetDraft,
    isDirty: form.formState.isDirty,
    label: stored
      ? undefined
      : t('common:actions.create'),
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

  return <Form
    onSubmit={onSubmit}
    validationBehavior='aria'
    className='flex max-w-2xl flex-col gap-4'
  >
    {/* Key */}
    <TextField
      isRequired
      id='environment-variable-key'
      isInvalid={Boolean(errors.key)}
      value={values.key}
      onChange={(value) => form.setValue('key', value, { shouldDirty: true, shouldValidate: true })}
    >
      <Label>{t('form.key')}</Label>
      <Input ref={keyRef} className='font-mono' autoComplete='off' spellCheck={false} />
      <Description>{t('form.keyHint')}</Description>
      <FieldError>{errors.key?.message}</FieldError>
    </TextField>

    {/* Secret */}
    <Switch
      id='environment-variable-secret'
      isSelected={isSecret}
      onChange={(isSelected) => form.setValue('isSecret', isSelected, { shouldDirty: true, shouldValidate: true })}
    >
      <Switch.Content>
        <Switch.Control>
          <Switch.Thumb />
        </Switch.Control>
        <Label>{t('form.isSecret')}</Label>
      </Switch.Content>
      <Description>{t('form.isSecretHint')}</Description>
    </Switch>

    {/* Value: a secret's is hidden while typed, with a toggle to show it */}
    {isSecret
      ? <SecretInput
        id='environment-variable-value'
        isRequired={!keepsStoredSecret}
        isMonospace
        label={t('form.value')}
        description={valueHint()}
        errorMessage={errors.value?.message}
        value={values.value ?? ''}
        onChange={(value) => form.setValue('value', value, { shouldDirty: true, shouldValidate: true })}
      />
      : <TextField
        id='environment-variable-value'
        isRequired={isRevealingSecret}
        isInvalid={Boolean(errors.value)}
        value={values.value}
        onChange={(value) => form.setValue('value', value, { shouldDirty: true, shouldValidate: true })}
      >
        <Label>{t('form.value')}</Label>
        <TextArea className='font-mono' rows={3} spellCheck={false} />
        <Description>{valueHint()}</Description>
        <FieldError>{errors.value?.message}</FieldError>
      </TextField>}

    {/* Description */}
    <TextField
      id='environment-variable-description'
      isInvalid={Boolean(errors.description)}
      value={values.description}
      onChange={(value) => form.setValue('description', value, { shouldDirty: true, shouldValidate: true })}
    >
      <Label>{t('form.description')}</Label>
      <TextArea rows={2} />
      <FieldError>{errors.description?.message}</FieldError>
    </TextField>

    {!stored && <PageCreateFooter />}
  </Form>
}
