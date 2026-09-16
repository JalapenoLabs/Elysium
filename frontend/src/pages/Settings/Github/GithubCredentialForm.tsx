// Copyright © 2026 Jalapeno Labs

import type { GithubCredential } from '../../../api/routes/githubRoutes'
import type { GithubFormInput, GithubFormValues } from './githubFormSchema'

// Core
import { useMemo } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { githubCredentialUpserted } from '../../../store/githubCredentialsSlice'

// User interface
import {
  Button,
  Description,
  FieldError,
  Form,
  Input,
  Label,
  ListBox,
  Select,
  TextField,
  toast,
} from '@heroui/react'
import { GithubSetupChecklist } from './GithubSetupChecklist'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'
import { HTTPError } from 'ky'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import {
  createGithubCredential,
  GITHUB_TOKEN_KINDS,
  updateGithubCredential,
} from '../../../api/routes/githubRoutes'
import { createGithubFormSchema } from './githubFormSchema'
import { githubKindLabelKeys, githubTokenLabelKeys } from './githubPresentation'

type Props = {
  // The credential being edited, or null to add a new one.
  credential: GithubCredential | null
  onSaved: () => void
  onCancel: () => void
}

// Adds or edits a GitHub token, with that kind's setup steps beside it. The token is
// write-only: when editing, a blank field keeps the stored one, unless the kind changed.
export function GithubCredentialForm(props: Props) {
  const { t } = useTranslation([ 'github', 'common' ])
  const dispatch = useAppDispatch()
  const stored = props.credential

  const resolver = useMemo(
    () => zodResolver(createGithubFormSchema(t, stored)),
    [ t, stored ],
  )

  const form = useForm<GithubFormInput, unknown, GithubFormValues>({
    resolver,
    defaultValues: {
      name: stored?.name ?? '',
      kind: stored?.kind ?? 'fine-grained',
      token: '',
    },
  })

  const onSubmit = form.handleSubmit(async (values) => {
    try {
      if (stored) {
        const response = await updateGithubCredential(stored.id, {
          name: values.name,
          kind: values.kind,
          // A blank token means "keep the stored one", so it is only sent when typed.
          token: values.token || undefined,
        })
        dispatch(githubCredentialUpserted(response.credential))
        toast.success(t('toasts.updated', { name: values.name }))
      }
      else {
        const response = await createGithubCredential({
          name: values.name,
          kind: values.kind,
          token: values.token,
        })
        dispatch(githubCredentialUpserted(response.credential))
        toast.success(t('toasts.created', { name: values.name }))
      }
      props.onSaved()
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 409) {
        form.setError('name', { message: t('form.errors.nameTaken') })
        return
      }

      // A token GitHub refuses comes back as a 400 naming what to check, which belongs on
      // the field the user can fix.
      const message = getApiErrorMessage(error)
      if (error instanceof HTTPError && error.response.status === 400 && message) {
        form.setError('token', { message })
        return
      }

      console.debug('GithubCredentialForm failed to save the credential', { error })
      toast.danger(t('common:errors.unexpected'), { description: message ?? undefined })
    }
  })

  const errors = form.formState.errors
  // useWatch subscribes per field and, unlike form.watch, is safe for the React Compiler.
  const values = useWatch({ control: form.control })
  const kind = values.kind ?? 'fine-grained'
  // The stored token belongs to the kind it was created as.
  const keepsStoredToken = stored !== null && stored.kind === kind

  return <div className='grid grid-cols-1 items-start gap-8 lg:grid-cols-[minmax(0,1fr)_minmax(0,24rem)]'>
    <Form onSubmit={onSubmit} validationBehavior='aria' className='flex flex-col gap-4'>
      {/* Name */}
      <TextField
        autoFocus
        isRequired
        isInvalid={Boolean(errors.name)}
        value={values.name}
        onChange={(value) => form.setValue('name', value, { shouldDirty: true, shouldValidate: true })}
      >
        <Label>{t('form.name')}</Label>
        <Input />
        <Description>{t('form.nameHint')}</Description>
        <FieldError>{errors.name?.message}</FieldError>
      </TextField>

      {/* Token type */}
      <Select
        isRequired
        value={kind}
        onChange={(key) => {
          const selected = GITHUB_TOKEN_KINDS.find((candidate) => candidate === key)
          if (!selected) {
            console.debug('GithubCredentialForm ignored an unknown token kind', { key })
            return
          }
          form.setValue('kind', selected, { shouldDirty: true, shouldValidate: true })
        }}
      >
        <Label>{t('form.kind')}</Label>
        <Select.Trigger>
          <Select.Value />
          <Select.Indicator />
        </Select.Trigger>
        <Select.Popover>
          <ListBox>{
            GITHUB_TOKEN_KINDS.map((candidate) => <ListBox.Item
              key={candidate}
              id={candidate}
              textValue={t(githubKindLabelKeys[candidate])}
            >
              {t(githubKindLabelKeys[candidate])}
              <ListBox.ItemIndicator />
            </ListBox.Item>)
          }</ListBox>
        </Select.Popover>
        <Description>{t('form.kindHint')}</Description>
      </Select>

      {/* Token */}
      <TextField
        isRequired={!keepsStoredToken}
        isInvalid={Boolean(errors.token)}
        type='password'
        autoComplete='off'
        value={values.token}
        onChange={(value) => form.setValue('token', value, { shouldDirty: true, shouldValidate: true })}
      >
        <Label>{t('form.token')}</Label>
        <Input />
        <Description>{
          keepsStoredToken
            ? t('form.tokenKeep')
            : t(githubTokenLabelKeys[kind].hint)
        }</Description>
        <FieldError>{errors.token?.message}</FieldError>
      </TextField>

      <p className='text-sm opacity-70'>{t('form.checkedHint')}</p>

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
      <GithubSetupChecklist
        // Remount per kind, so checked steps never carry over to the other kind's list.
        key={kind}
        kind={kind}
      />
    </aside>
  </div>
}
