// Copyright © 2026 Jalapeno Labs

import type { StorageLocation } from '../../../api/routes/storageRoutes'
import type { StorageFormInput, StorageFormValues } from './storageFormSchema'

// Core
import { useMemo } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { storageLocationUpserted } from '../../../store/storageLocationsSlice'

// User interface
import {
  Button,
  Description,
  FieldError,
  Form,
  Input,
  Label,
  ListBox,
  NumberField,
  Select,
  Switch,
  TextField,
  toast,
} from '@heroui/react'
import { ProjectScopePicker } from '../../../components/ProjectScopePicker'
import { StorageSetupChecklist } from './StorageSetupChecklist'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'
import { HTTPError } from 'ky'

// Misc
import { getUpstreamErrorMessage } from '../../../api/errors'
import {
  BUNNY_STORAGE_REGIONS,
  createStorageLocation,
  updateStorageLocation,
} from '../../../api/routes/storageRoutes'
import { BYTES_PER_GIGABYTE } from '../../../constants'
import { createStorageFormSchema, keepsAccessKey, toStorageProvider } from './storageFormSchema'
import {
  accessKeyLabelKeys,
  bunnyRegionLabelKeys,
  getStorageOption,
  STORAGE_OPTIONS,
  storageOptionLabelKeys,
} from './storagePresentation'

// A new AWS bucket's region until one is typed: AWS's default region.
const DEFAULT_AWS_REGION = 'us-east-1'

type Props = {
  // The location being edited, or null to add a new one.
  location: StorageLocation | null
  onSaved: (location: StorageLocation) => void
  onCancel: () => void
}

// Adds or edits a storage location, with the chosen provider's setup steps beside it. The
// secret is write-only: when editing, a blank field keeps the stored one, unless the new
// settings need their own.
export function StorageLocationForm(props: Props) {
  const { t } = useTranslation([ 'storage', 'common' ])
  const dispatch = useAppDispatch()
  const stored = props.location?.provider ?? null

  const resolver = useMemo(
    () => zodResolver(createStorageFormSchema(t, stored)),
    [ t, stored ],
  )

  const form = useForm<StorageFormInput, unknown, StorageFormValues>({
    resolver,
    defaultValues: {
      name: props.location?.name ?? '',
      option: stored
        ? getStorageOption(stored)
        : 'bunny',
      zone: stored?.kind === 'bunny'
        ? stored.zone
        : '',
      bunnyRegion: stored?.kind === 'bunny'
        ? stored.region
        : 'frankfurt',
      bucket: stored?.kind === 's3'
        ? stored.bucket
        : '',
      awsRegion: stored?.kind === 's3' && stored.region
        ? stored.region
        : DEFAULT_AWS_REGION,
      accessKeyId: stored?.kind === 's3'
        ? stored.accessKeyId
        : '',
      pathPrefix: props.location?.pathPrefix ?? '',
      projects: props.location?.projects ?? [],
      isUnlimited: props.location
        ? props.location.storageLimitBytes === null
        : false,
      // A new location starts with the field empty, so a limit is chosen rather than accepted.
      storageLimitGigabytes: props.location?.storageLimitBytes
        ? props.location.storageLimitBytes / BYTES_PER_GIGABYTE
        : Number.NaN,
      accessKey: '',
    },
  })

  const onSubmit = form.handleSubmit(async (values) => {
    const payload = {
      name: values.name,
      provider: toStorageProvider(values),
      pathPrefix: values.pathPrefix,
      projects: values.projects,
      storageLimitBytes: values.isUnlimited
        ? null
        : Math.round(values.storageLimitGigabytes * BYTES_PER_GIGABYTE),
    }

    try {
      if (props.location) {
        // A blank secret means "keep the stored one", so it is only sent when typed.
        const accessKey = values.accessKey.trim()
          ? values.accessKey
          : undefined
        const response = await updateStorageLocation(props.location.id, { ...payload, accessKey })
        dispatch(storageLocationUpserted(response.location))
        toast.success(t('toasts.updated', { name: values.name }))
        props.onSaved(response.location)
      }
      else {
        const response = await createStorageLocation({ ...payload, accessKey: values.accessKey })
        dispatch(storageLocationUpserted(response.location))
        toast.success(t('toasts.created', { name: values.name }))
        props.onSaved(response.location)
      }
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 409) {
        form.setError('name', { message: t('form.errors.nameTaken') })
        return
      }

      console.debug('StorageLocationForm failed to save the location', { error })
      toast.danger(t('common:errors.unexpected'), {
        description: getUpstreamErrorMessage(error) ?? undefined,
      })
    }
  })

  const errors = form.formState.errors
  // useWatch subscribes per field and, unlike form.watch, is safe for the React Compiler.
  const values = useWatch({ control: form.control })
  const option = values.option ?? 'bunny'
  const labels = accessKeyLabelKeys[option]

  // Whether the stored secret still opens what the fields now describe.
  const keepsStoredSecret = stored !== null && keepsAccessKey(
    toStorageProvider({
      option,
      zone: values.zone ?? '',
      bunnyRegion: values.bunnyRegion ?? 'frankfurt',
      bucket: values.bucket ?? '',
      awsRegion: values.awsRegion ?? '',
      accessKeyId: values.accessKeyId ?? '',
    }),
    stored,
  )

  function setText(field: 'name' | 'zone' | 'bucket' | 'awsRegion' | 'accessKeyId' | 'pathPrefix' | 'accessKey') {
    return (value: string) => form.setValue(field, value, { shouldDirty: true, shouldValidate: true })
  }

  const bucketField = <TextField
    isRequired
    isInvalid={Boolean(errors.bucket)}
    autoComplete='off'
    value={values.bucket}
    onChange={setText('bucket')}
  >
    <Label>{t('form.bucket')}</Label>
    <Input className='font-mono' spellCheck={false} />
    <Description>{t('form.bucketHint')}</Description>
    <FieldError>{errors.bucket?.message}</FieldError>
  </TextField>

  return <div className='grid grid-cols-1 items-start gap-8 lg:grid-cols-[minmax(0,1fr)_minmax(0,24rem)]'>
    <Form onSubmit={onSubmit} validationBehavior='aria' className='flex flex-col gap-4'>
      {/* Name */}
      <TextField
        autoFocus
        isRequired
        isInvalid={Boolean(errors.name)}
        value={values.name}
        onChange={setText('name')}
      >
        <Label>{t('form.name')}</Label>
        <Input />
        <FieldError>{errors.name?.message}</FieldError>
      </TextField>

      {/* Provider */}
      <Select
        isRequired
        value={option}
        onChange={(key) => {
          const selected = STORAGE_OPTIONS.find((candidate) => candidate === key)
          if (!selected) {
            console.debug('StorageLocationForm ignored an unknown provider', { key })
            return
          }
          form.setValue('option', selected, { shouldDirty: true, shouldValidate: form.formState.isSubmitted })
        }}
      >
        <Label>{t('form.provider')}</Label>
        <Select.Trigger>
          <Select.Value />
          <Select.Indicator />
        </Select.Trigger>
        <Select.Popover>
          <ListBox>{
            STORAGE_OPTIONS.map((candidate) => <ListBox.Item
              key={candidate}
              id={candidate}
              textValue={t(storageOptionLabelKeys[candidate])}
            >
              {t(storageOptionLabelKeys[candidate])}
              <ListBox.ItemIndicator />
            </ListBox.Item>)
          }</ListBox>
        </Select.Popover>
      </Select>

      {option === 'bunny' && <div className='grid grid-cols-1 gap-4 sm:grid-cols-2'>
        {/* Zone */}
        <TextField
          isRequired
          isInvalid={Boolean(errors.zone)}
          autoComplete='off'
          value={values.zone}
          onChange={setText('zone')}
        >
          <Label>{t('form.zone')}</Label>
          <Input className='font-mono' spellCheck={false} />
          <Description>{t('form.zoneHint')}</Description>
          <FieldError>{errors.zone?.message}</FieldError>
        </TextField>

        {/* Bunny region */}
        <Select
          isRequired
          value={values.bunnyRegion}
          onChange={(key) => {
            const selected = BUNNY_STORAGE_REGIONS.find((candidate) => candidate === key)
            if (!selected) {
              console.debug('StorageLocationForm ignored an unknown region', { key })
              return
            }
            form.setValue('bunnyRegion', selected, { shouldDirty: true })
          }}
        >
          <Label>{t('form.region')}</Label>
          <Select.Trigger>
            <Select.Value />
            <Select.Indicator />
          </Select.Trigger>
          <Select.Popover>
            <ListBox>{
              BUNNY_STORAGE_REGIONS.map((candidate) => <ListBox.Item
                key={candidate}
                id={candidate}
                textValue={t(bunnyRegionLabelKeys[candidate])}
              >
                {t(bunnyRegionLabelKeys[candidate])}
                <ListBox.ItemIndicator />
              </ListBox.Item>)
            }</ListBox>
          </Select.Popover>
          <Description>{t('form.regionHint')}</Description>
        </Select>
      </div>}

      {option === 'aws' && <div className='grid grid-cols-1 gap-4 sm:grid-cols-2'>
        {bucketField}

        {/* AWS region */}
        <TextField
          isRequired
          isInvalid={Boolean(errors.awsRegion)}
          autoComplete='off'
          value={values.awsRegion}
          onChange={setText('awsRegion')}
        >
          <Label>{t('form.region')}</Label>
          <Input className='font-mono' spellCheck={false} placeholder={DEFAULT_AWS_REGION} />
          <Description>{t('form.awsRegionHint')}</Description>
          <FieldError>{errors.awsRegion?.message}</FieldError>
        </TextField>
      </div>}

      {option === 'google-cloud' && bucketField}

      {/* Directory */}
      <TextField
        isInvalid={Boolean(errors.pathPrefix)}
        autoComplete='off'
        value={values.pathPrefix}
        onChange={setText('pathPrefix')}
      >
        <Label>{t('form.pathPrefix')}</Label>
        <Input className='font-mono' spellCheck={false} placeholder='elysium/uploads' />
        <Description>{t('form.pathPrefixHint')}</Description>
        <FieldError>{errors.pathPrefix?.message}</FieldError>
      </TextField>

      {/* Projects */}
      <ProjectScopePicker
        label={t('form.projects')}
        description={t('form.projectsHint')}
        value={values.projects ?? []}
        onChange={(value) => form.setValue('projects', value, { shouldDirty: true })}
      />

      {/* Storage limit, or none */}
      <div className='flex items-start gap-4'>
        <NumberField
          className='min-w-0 flex-1'
          isRequired={!values.isUnlimited}
          isDisabled={values.isUnlimited}
          isInvalid={!values.isUnlimited && Boolean(errors.storageLimitGigabytes)}
          minValue={0}
          step={1}
          formatOptions={{ style: 'unit', unit: 'gigabyte', maximumFractionDigits: 3 }}
          value={values.storageLimitGigabytes}
          onChange={(value) => form.setValue(
            'storageLimitGigabytes',
            value,
            { shouldDirty: true, shouldValidate: true },
          )}
        >
          <Label>{t('form.storageLimit')}</Label>
          <NumberField.Group>
            <NumberField.DecrementButton />
            <NumberField.Input />
            <NumberField.IncrementButton />
          </NumberField.Group>
          <Description>{
            values.isUnlimited
              ? t('form.noLimitHint')
              : t('form.storageLimitHint')
          }</Description>
          <FieldError>{errors.storageLimitGigabytes?.message}</FieldError>
        </NumberField>
        <Switch
          className='mt-8 shrink-0'
          isSelected={values.isUnlimited}
          onChange={(isSelected) => form.setValue(
            'isUnlimited',
            isSelected,
            { shouldDirty: true, shouldValidate: true },
          )}
        >
          <Switch.Control>
            <Switch.Thumb />
          </Switch.Control>
          <Switch.Content>
            <Label>{t('form.noLimit')}</Label>
          </Switch.Content>
        </Switch>
      </div>

      {/* Access key id, for S3 */}
      {labels.keyId && <TextField
        isRequired
        isInvalid={Boolean(errors.accessKeyId)}
        autoComplete='off'
        value={values.accessKeyId}
        onChange={setText('accessKeyId')}
      >
        <Label>{t(labels.keyId)}</Label>
        <Input className='font-mono' spellCheck={false} />
        <Description>{t(labels.keyIdHint)}</Description>
        <FieldError>{errors.accessKeyId?.message}</FieldError>
      </TextField>}

      {/* Secret */}
      <TextField
        isRequired={!keepsStoredSecret}
        isInvalid={Boolean(errors.accessKey)}
        type='password'
        autoComplete='off'
        value={values.accessKey}
        onChange={setText('accessKey')}
      >
        <Label>{t(labels.secret)}</Label>
        <Input />
        <Description>{
          keepsStoredSecret
            ? t('form.accessKeyKeep')
            : t(labels.secretHint)
        }</Description>
        <FieldError>{errors.accessKey?.message}</FieldError>
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
      <StorageSetupChecklist
        // Remount per option, so checked steps never carry over to another provider's list.
        key={option}
        option={option}
      />
    </aside>
  </div>
}
