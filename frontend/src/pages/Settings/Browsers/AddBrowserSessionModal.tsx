// Copyright © 2026 Jalapeno Labs

import type { UseOverlayStateReturn } from '@heroui/react'
import type { BrowserFormValues } from './browserFormSchema'
import type { BrowserPreset } from './browserPresets'

// Core
import { useMemo, useState } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// Redux
import { browserSessionUpserted } from '../../../store/browserSessionsSlice'
import { useAppDispatch } from '../../../store/hooks'

// User interface
import {
  Button,
  FieldError,
  Form,
  Input,
  Label,
  ListBox,
  Modal,
  Select,
  TextField,
  toast,
} from '@heroui/react'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'
import { HTTPError } from 'ky'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { createBrowserSession } from '../../../api/routes/browserSessionRoutes'
import { getBrowserSessionUrl } from '../../../urls'
import { createBrowserFormSchema } from './browserFormSchema'
import { BROWSER_PRESETS, startUrlByPreset } from './browserPresets'

type Props = {
  state: UseOverlayStateReturn
}

export function AddBrowserSessionModal(props: Props) {
  const { t } = useTranslation([ 'browsers', 'common' ])
  const dispatch = useAppDispatch()
  const navigate = useNavigate()
  const [ preset, setPreset ] = useState<BrowserPreset>('outlook')

  const resolver = useMemo(
    () => zodResolver(createBrowserFormSchema(t)),
    [ t ],
  )

  const form = useForm<BrowserFormValues>({
    resolver,
    defaultValues: {
      name: t('presets.outlook.name'),
      startUrl: startUrlByPreset.outlook,
    },
  })

  const onSubmit = form.handleSubmit(async (values) => {
    try {
      const response = await createBrowserSession(values)
      dispatch(browserSessionUpserted(response.browserSession))
      props.state.close()
      // Straight to the stream: the browser comes up there, ready to sign in.
      navigate(getBrowserSessionUrl(response.browserSession.id))
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 409) {
        toast.danger(t('form.errors.portsExhausted'))
        return
      }
      console.debug('AddBrowserSessionModal failed to add the browser', { error })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
  })

  const errors = form.formState.errors
  const [ name, startUrl ] = useWatch({
    control: form.control,
    name: [ 'name', 'startUrl' ],
  })

  function choosePreset(chosen: BrowserPreset) {
    setPreset(chosen)
    if (chosen === 'other') {
      form.setValue('name', '', { shouldDirty: true })
      form.setValue('startUrl', '', { shouldDirty: true })
      return
    }
    form.setValue('name', t(`presets.${chosen}.name`), { shouldDirty: true, shouldValidate: true })
    form.setValue('startUrl', startUrlByPreset[chosen], { shouldDirty: true, shouldValidate: true })
  }

  // Controlled overlays skip the Modal root: it is a trigger wrapper, and without a
  // pressable child React Aria warns. The backdrop takes the open state directly.
  return <Modal.Backdrop isOpen={props.state.isOpen} onOpenChange={props.state.setOpen}>
    <Modal.Container>
      <Modal.Dialog className='sm:max-w-md'>
        <Modal.CloseTrigger />
        <Modal.Header>
          <Modal.Heading>{
            t('form.title')
          }</Modal.Heading>
          <p className='mt-1 text-sm opacity-70'>{
            t('form.description')
          }</p>
        </Modal.Header>
        <Form onSubmit={onSubmit} validationBehavior='aria'>
          <Modal.Body className='mt-2 flex flex-col gap-4'>
            <Select
              value={preset}
              onChange={(key) => {
                const chosen = BROWSER_PRESETS.find((option) => option === key)
                if (!chosen) {
                  console.debug('AddBrowserSessionModal ignored an unknown service', { key })
                  return
                }
                choosePreset(chosen)
              }}
            >
              <Label>{t('form.service')}</Label>
              <Select.Trigger>
                <Select.Value />
                <Select.Indicator />
              </Select.Trigger>
              <Select.Popover>
                <ListBox>{
                  BROWSER_PRESETS.map((option) => <ListBox.Item
                    key={option}
                    id={option}
                    textValue={t(`presets.${option}.label`)}
                  >
                    {t(`presets.${option}.label`)}
                    <ListBox.ItemIndicator />
                  </ListBox.Item>)
                }</ListBox>
              </Select.Popover>
            </Select>
            <TextField
              isRequired
              autoComplete='off'
              isInvalid={Boolean(errors.name)}
              value={name}
              onChange={(value) => form.setValue('name', value, { shouldDirty: true, shouldValidate: true })}
            >
              <Label>{t('form.name')}</Label>
              <Input />
              <FieldError>{errors.name?.message}</FieldError>
            </TextField>
            <TextField
              isRequired
              autoComplete='off'
              isInvalid={Boolean(errors.startUrl)}
              value={startUrl}
              onChange={(value) => form.setValue('startUrl', value, { shouldDirty: true, shouldValidate: true })}
            >
              <Label>{t('form.startUrl')}</Label>
              <Input placeholder='https://' />
              <FieldError>{errors.startUrl?.message}</FieldError>
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
              <span>{t('form.submit')}</span>
            </Button>
          </Modal.Footer>
        </Form>
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>
}
