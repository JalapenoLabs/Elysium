// Copyright © 2026 Jalapeno Labs

import type { UseOverlayStateReturn } from '@heroui/react'
import type { Satellite } from '../../../api/routes/satelliteRoutes'
import type { SatelliteFormValues } from './satelliteFormSchema'

// Core
import { useMemo } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { satelliteUpserted } from '../../../store/satellitesSlice'

// User interface
import {
  Button,
  Description,
  FieldError,
  Form,
  Input,
  Label,
  Modal,
  Switch,
  TextArea,
  TextField,
  toast,
} from '@heroui/react'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'
import { HTTPError } from 'ky'

// Misc
import { createSatellite, updateSatellite } from '../../../api/routes/satelliteRoutes'
import { createSatelliteFormSchema } from './satelliteFormSchema'

type Props = {
  state: UseOverlayStateReturn
  // The satellite being edited, or null to register a new one.
  satellite: Satellite | null
}

export function SatelliteFormModal(props: Props) {
  const { t } = useTranslation([ 'satellites', 'common' ])
  const dispatch = useAppDispatch()
  const mode = props.satellite
    ? 'edit'
    : 'create'

  const resolver = useMemo(
    () => zodResolver(createSatelliteFormSchema(t, mode)),
    [ t, mode ],
  )

  const form = useForm<SatelliteFormValues>({
    resolver,
    defaultValues: {
      name: props.satellite?.name ?? '',
      description: props.satellite?.description ?? '',
      url: props.satellite?.url ?? '',
      secret: '',
      isActive: props.satellite?.isActive ?? true,
    },
  })

  const onSubmit = form.handleSubmit(async (values) => {
    const payload = {
      name: values.name,
      description: values.description,
      url: values.url,
      isActive: values.isActive,
    }

    try {
      if (props.satellite) {
        // A blank secret means "keep the stored one", so it is only sent when typed.
        const secret = values.secret.trim()
          ? values.secret
          : undefined
        const response = await updateSatellite(props.satellite.id, { ...payload, secret })
        dispatch(satelliteUpserted(response.satellite))
        toast.success(t('toasts.updated', { name: values.name }))
      }
      else {
        const response = await createSatellite({ ...payload, secret: values.secret })
        dispatch(satelliteUpserted(response.satellite))
        toast.success(t('toasts.created', { name: values.name }))
      }

      props.state.close()
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 409) {
        form.setError('name', { message: t('form.errors.nameTaken') })
        return
      }

      console.debug('SatelliteFormModal failed to save the satellite', { error })
      toast.danger(t('common:errors.unexpected'))
    }
  })

  const errors = form.formState.errors
  // useWatch subscribes per field and, unlike form.watch, is safe for the React Compiler.
  const [ name, description, url, secret, isActive ] = useWatch({
    control: form.control,
    name: [ 'name', 'description', 'url', 'secret', 'isActive' ],
  })

  // Controlled overlays skip the Modal root: it is a trigger wrapper, and without a
  // pressable child React Aria warns. The backdrop takes the open state directly.
  return <Modal.Backdrop isOpen={props.state.isOpen} onOpenChange={props.state.setOpen}>
    <Modal.Container>
      <Modal.Dialog className='sm:max-w-lg'>
        <Modal.CloseTrigger />
        <Modal.Header>
          <Modal.Heading>{
            props.satellite
              ? t('form.editTitle', { name: props.satellite.name })
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

            {/* Description */}
            <TextField
              isInvalid={Boolean(errors.description)}
              value={description}
              onChange={(value) => form.setValue('description', value, { shouldDirty: true, shouldValidate: true })}
            >
              <Label>{t('form.description')}</Label>
              <TextArea rows={2} />
              <FieldError>{errors.description?.message}</FieldError>
            </TextField>

            {/* URL */}
            <TextField
              isRequired
              isInvalid={Boolean(errors.url)}
              type='url'
              value={url}
              onChange={(value) => form.setValue('url', value, { shouldDirty: true, shouldValidate: true })}
            >
              <Label>{t('form.url')}</Label>
              <Input placeholder='http://arsox:8080' />
              <Description>{t('form.urlHint')}</Description>
              <FieldError>{errors.url?.message}</FieldError>
            </TextField>

            {/* Secret */}
            <TextField
              isRequired={mode === 'create'}
              isInvalid={Boolean(errors.secret)}
              type='password'
              autoComplete='off'
              value={secret}
              onChange={(value) => form.setValue('secret', value, { shouldDirty: true, shouldValidate: true })}
            >
              <Label>{t('form.secret')}</Label>
              <Input />
              <Description>{
                mode === 'edit'
                  ? t('form.secretKeep')
                  : t('form.secretHint')
              }</Description>
              <FieldError>{errors.secret?.message}</FieldError>
            </TextField>

            {/* Active */}
            <Switch
              isSelected={isActive}
              onChange={(isSelected) => form.setValue('isActive', isSelected, { shouldDirty: true })}
            >
              <Switch.Control>
                <Switch.Thumb />
              </Switch.Control>
              <Switch.Content>
                <Label>{t('form.isActive')}</Label>
              </Switch.Content>
            </Switch>
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
