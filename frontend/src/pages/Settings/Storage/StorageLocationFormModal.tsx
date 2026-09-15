// Copyright © 2026 Jalapeno Labs

import type { UseOverlayStateReturn } from '@heroui/react'
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
  Modal,
  NumberField,
  Select,
  Switch,
  TextField,
  toast,
} from '@heroui/react'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'
import { HTTPError } from 'ky'

// Misc
import {
  BUNNY_STORAGE_REGIONS,
  createStorageLocation,
  STORAGE_PROVIDER_KINDS,
  updateStorageLocation,
} from '../../../api/routes/storageRoutes'
import { BYTES_PER_GIGABYTE } from '../../../constants'
import { createStorageFormSchema } from './storageFormSchema'
import { bunnyRegionLabelKeys, storageProviderLabelKeys } from './storagePresentation'

type Props = {
  state: UseOverlayStateReturn
  // The location being edited, or null to add a new one.
  location: StorageLocation | null
}

export function StorageLocationFormModal(props: Props) {
  const { t } = useTranslation([ 'storage', 'common' ])
  const dispatch = useAppDispatch()
  const mode = props.location
    ? 'edit'
    : 'create'

  const resolver = useMemo(
    () => zodResolver(createStorageFormSchema(t, mode)),
    [ t, mode ],
  )

  const form = useForm<StorageFormInput, unknown, StorageFormValues>({
    resolver,
    defaultValues: {
      name: props.location?.name ?? '',
      kind: 'bunny',
      zone: props.location?.provider.zone ?? '',
      region: props.location?.provider.region ?? 'frankfurt',
      pathPrefix: props.location?.pathPrefix ?? '',
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
      provider: {
        kind: values.kind,
        zone: values.zone,
        region: values.region,
      },
      pathPrefix: values.pathPrefix,
      storageLimitBytes: values.isUnlimited
        ? null
        : Math.round(values.storageLimitGigabytes * BYTES_PER_GIGABYTE),
    }

    try {
      if (props.location) {
        // A blank access key means "keep the stored one", so it is only sent when typed.
        const accessKey = values.accessKey.trim()
          ? values.accessKey
          : undefined
        const response = await updateStorageLocation(props.location.id, { ...payload, accessKey })
        dispatch(storageLocationUpserted(response.location))
        toast.success(t('toasts.updated', { name: values.name }))
      }
      else {
        const response = await createStorageLocation({ ...payload, accessKey: values.accessKey })
        dispatch(storageLocationUpserted(response.location))
        toast.success(t('toasts.created', { name: values.name }))
      }

      props.state.close()
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 409) {
        form.setError('name', { message: t('form.errors.nameTaken') })
        return
      }

      console.debug('StorageLocationFormModal failed to save the location', { error })
      toast.danger(t('common:errors.unexpected'))
    }
  })

  const errors = form.formState.errors
  // useWatch subscribes per field and, unlike form.watch, is safe for the React Compiler.
  const [ name, kind, zone, region, pathPrefix, isUnlimited, storageLimitGigabytes, accessKey ] = useWatch({
    control: form.control,
    name: [ 'name', 'kind', 'zone', 'region', 'pathPrefix', 'isUnlimited', 'storageLimitGigabytes', 'accessKey' ],
  })

  // Controlled overlays skip the Modal root: it is a trigger wrapper, and without a
  // pressable child React Aria warns. The backdrop takes the open state directly.
  return <Modal.Backdrop isOpen={props.state.isOpen} onOpenChange={props.state.setOpen}>
    <Modal.Container>
      <Modal.Dialog className='sm:max-w-xl'>
        <Modal.CloseTrigger />
        <Modal.Header>
          <Modal.Heading>{
            props.location
              ? t('form.editTitle', { name: props.location.name })
              : t('form.createTitle')
          }</Modal.Heading>
        </Modal.Header>
        <Form onSubmit={onSubmit} validationBehavior='aria'>
          <Modal.Body className='mt-2 flex flex-col gap-4'>
            {/* Name */}
            <TextField
              autoFocus
              isRequired
              isInvalid={Boolean(errors.name)}
              value={name}
              onChange={(value) => form.setValue('name', value, { shouldDirty: true, shouldValidate: true })}
            >
              <Label>{t('form.name')}</Label>
              <Input />
              <FieldError>{errors.name?.message}</FieldError>
            </TextField>

            {/* Provider */}
            <Select
              isRequired
              value={kind}
              onChange={(key) => {
                const selected = STORAGE_PROVIDER_KINDS.find((candidate) => candidate === key)
                if (!selected) {
                  console.debug('StorageLocationFormModal ignored an unknown provider', { key })
                  return
                }
                form.setValue('kind', selected, { shouldDirty: true })
              }}
            >
              <Label>{t('form.provider')}</Label>
              <Select.Trigger>
                <Select.Value />
                <Select.Indicator />
              </Select.Trigger>
              <Select.Popover>
                <ListBox>{
                  STORAGE_PROVIDER_KINDS.map((candidate) => <ListBox.Item
                    key={candidate}
                    id={candidate}
                    textValue={t(storageProviderLabelKeys[candidate])}
                  >
                    {t(storageProviderLabelKeys[candidate])}
                    <ListBox.ItemIndicator />
                  </ListBox.Item>)
                }</ListBox>
              </Select.Popover>
            </Select>

            <div className='grid grid-cols-1 gap-4 sm:grid-cols-2'>
              {/* Zone */}
              <TextField
                isRequired
                isInvalid={Boolean(errors.zone)}
                autoComplete='off'
                value={zone}
                onChange={(value) => form.setValue('zone', value, { shouldDirty: true, shouldValidate: true })}
              >
                <Label>{t('form.zone')}</Label>
                <Input className='font-mono' spellCheck={false} />
                <Description>{t('form.zoneHint')}</Description>
                <FieldError>{errors.zone?.message}</FieldError>
              </TextField>

              {/* Region */}
              <Select
                isRequired
                value={region}
                onChange={(key) => {
                  const selected = BUNNY_STORAGE_REGIONS.find((candidate) => candidate === key)
                  if (!selected) {
                    console.debug('StorageLocationFormModal ignored an unknown region', { key })
                    return
                  }
                  form.setValue('region', selected, { shouldDirty: true })
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
            </div>

            {/* Directory */}
            <TextField
              isInvalid={Boolean(errors.pathPrefix)}
              autoComplete='off'
              value={pathPrefix}
              onChange={(value) => form.setValue('pathPrefix', value, { shouldDirty: true, shouldValidate: true })}
            >
              <Label>{t('form.pathPrefix')}</Label>
              <Input className='font-mono' spellCheck={false} placeholder='elysium/uploads' />
              <Description>{t('form.pathPrefixHint')}</Description>
              <FieldError>{errors.pathPrefix?.message}</FieldError>
            </TextField>

            {/* Storage limit, or none */}
            <div className='flex items-start gap-4'>
              <NumberField
                className='min-w-0 flex-1'
                isRequired={!isUnlimited}
                isDisabled={isUnlimited}
                isInvalid={!isUnlimited && Boolean(errors.storageLimitGigabytes)}
                minValue={0}
                step={1}
                formatOptions={{ style: 'unit', unit: 'gigabyte', maximumFractionDigits: 3 }}
                value={storageLimitGigabytes}
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
                  isUnlimited
                    ? t('form.noLimitHint')
                    : t('form.storageLimitHint')
                }</Description>
                <FieldError>{errors.storageLimitGigabytes?.message}</FieldError>
              </NumberField>
              <Switch
                className='mt-8 shrink-0'
                isSelected={isUnlimited}
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

            {/* Access key */}
            <TextField
              isRequired={mode === 'create'}
              isInvalid={Boolean(errors.accessKey)}
              type='password'
              autoComplete='off'
              value={accessKey}
              onChange={(value) => form.setValue('accessKey', value, { shouldDirty: true, shouldValidate: true })}
            >
              <Label>{t('form.accessKey')}</Label>
              <Input />
              <Description>{
                mode === 'edit'
                  ? t('form.accessKeyKeep')
                  : t('form.accessKeyHint')
              }</Description>
              <FieldError>{errors.accessKey?.message}</FieldError>
            </TextField>
          </Modal.Body>
          <Modal.Footer>
            <Button slot='close' variant='tertiary'>
              <span>{t('common:actions.cancel')}</span>
            </Button>
            <Button
              type='submit'
              isPending={form.formState.isSubmitting}
            >
              <span>{t('common:actions.save')}</span>
            </Button>
          </Modal.Footer>
        </Form>
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>
}
