// Copyright © 2026 Jalapeno Labs

import type { StorageLocation } from '../../../api/routes/storageRoutes'
import type { FirefliesFormInput, FirefliesFormValues } from './firefliesFormSchema'

// Core
import { useMemo, useState } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// Redux
import { connectorAccountUpserted } from '../../../store/connectorAccountsSlice'
import { useAppDispatch } from '../../../store/hooks'

// User interface
import {
  Alert,
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

// Utility
import { zodResolver } from '@hookform/resolvers/zod'
import { HTTPError } from 'ky'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { createConnectorAccount } from '../../../api/routes/connectorRoutes'
import { createFirefliesFormSchema } from './firefliesFormSchema'

type Props = {
  storageLocations: StorageLocation[]
}

// Connects Fireflies with the person's own API key. The API checks the key with Fireflies
// before it stores anything, so a refusal comes back in Fireflies' words above the form.
export function FirefliesConnectForm(props: Props) {
  const { t } = useTranslation([ 'meetings', 'common' ])
  const dispatch = useAppDispatch()
  const [ refusal, setRefusal ] = useState<string | null>(null)

  const resolver = useMemo(() => zodResolver(createFirefliesFormSchema(t)), [ t ])
  const form = useForm<FirefliesFormInput, unknown, FirefliesFormValues>({
    resolver,
    defaultValues: {
      apiKey: '',
      storageLocationId: props.storageLocations.length === 1
        ? props.storageLocations[0].id
        : '',
    },
  })

  const onSubmit = form.handleSubmit(async (values) => {
    setRefusal(null)
    try {
      const response = await createConnectorAccount({
        kind: 'fireflies',
        apiKey: values.apiKey,
        storageLocationId: values.storageLocationId,
      })
      dispatch(connectorAccountUpserted(response.connector))
      toast.success(t('fireflies.connectedToast'))
    }
    catch (error) {
      const message = getApiErrorMessage(error)
      // Fireflies' refusal, or Fireflies not answering, says what to fix.
      if (error instanceof HTTPError && [ 400, 502 ].includes(error.response.status) && message) {
        setRefusal(message)
        return
      }
      console.debug('FirefliesConnectForm failed to connect Fireflies', { error })
      toast.danger(t('common:errors.unexpected'), { description: message ?? undefined })
    }
  })

  const errors = form.formState.errors
  const values = useWatch({ control: form.control })

  if (!props.storageLocations.length) {
    return <p className='rounded-xl border border-separator px-4 py-6 text-sm opacity-70'>{
      t('form.noStorageLocations')
    }</p>
  }

  return <Form onSubmit={onSubmit} validationBehavior='aria' className='flex max-w-xl flex-col gap-4'>
    <h3 className='text-base font-semibold'>{t('form.heading')}</h3>

    {refusal && <Alert status='danger'>
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Title>{t('form.refusedTitle')}</Alert.Title>
        <Alert.Description>{refusal}</Alert.Description>
      </Alert.Content>
    </Alert>}

    {/* API key */}
    <TextField
      autoFocus
      isRequired
      isInvalid={Boolean(errors.apiKey)}
      type='password'
      autoComplete='off'
      value={values.apiKey}
      onChange={(value) => form.setValue('apiKey', value, { shouldDirty: true, shouldValidate: true })}
    >
      <Label>{t('form.apiKey')}</Label>
      <Input />
      <Description>{t('form.apiKeyHint')}</Description>
      <FieldError>{errors.apiKey?.message}</FieldError>
    </TextField>

    {/* Where recordings are kept */}
    <Select
      isRequired
      isInvalid={Boolean(errors.storageLocationId)}
      value={values.storageLocationId || null}
      onChange={(key) => {
        const location = props.storageLocations.find((candidate) => candidate.id === key)
        if (!location) {
          console.debug('FirefliesConnectForm ignored an unknown storage location', { key })
          return
        }
        form.setValue('storageLocationId', location.id, { shouldDirty: true, shouldValidate: true })
      }}
    >
      <Label>{t('form.storageLocation')}</Label>
      <Select.Trigger>
        <Select.Value />
        <Select.Indicator />
      </Select.Trigger>
      <Select.Popover>
        <ListBox>{
          props.storageLocations.map((location) => <ListBox.Item
            key={location.id}
            id={location.id}
            textValue={location.name}
          >
            {location.name}
            <ListBox.ItemIndicator />
          </ListBox.Item>)
        }</ListBox>
      </Select.Popover>
      <Description>{t('form.storageLocationHint')}</Description>
      <FieldError>{errors.storageLocationId?.message}</FieldError>
    </Select>

    <div className='flex justify-end'>
      <Button type='submit' isPending={form.formState.isSubmitting}>
        <span>{t('form.connect')}</span>
      </Button>
    </div>
  </Form>
}
