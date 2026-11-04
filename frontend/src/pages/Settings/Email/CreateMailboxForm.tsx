// Copyright © 2026 Jalapeno Labs

import type { MailDomain } from '../../../api/routes/mailRoutes'
import type { CreateMailboxFormValues } from './mailFormSchemas'

// Core
import { useMemo } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { mailAccountUpserted } from '../../../store/mailAccountsSlice'

// User interface
import {
  FieldError,
  Form,
  Input,
  Label,
  ListBox,
  Select,
  TextField,
} from '@heroui/react'
import { PageCreateFooter } from '../../../layout/page/PageCreateFooter'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'
import { HTTPError } from 'ky'

// Misc
import { useFormDraft } from '../../../drafts/useFormDraft'
import { getUpstreamErrorMessage } from '../../../api/errors'
import { createMailbox } from '../../../api/routes/mailRoutes'
import { useInitialFocus } from '../../../hooks/useInitialFocus'
import { saveForm, usePageSave } from '../../../layout/page/pageSave'
import { UrlTree } from '../../../urls'
import { createMailboxFormSchema } from './mailFormSchemas'
import { toast } from '../../../toast/toast'

type Props = {
  // The mail server's domains; there is at least one.
  domains: MailDomain[]
}

// A mailbox on Elysium's own mail server: its address, on one of the server's domains, and the
// name it sends as. Creating it goes back to Email, which lists it.
export function CreateMailboxForm(props: Props) {
  const { t } = useTranslation([ 'email', 'common' ])
  const dispatch = useAppDispatch()
  const navigate = useNavigate()

  const resolver = useMemo(
    () => zodResolver(createMailboxFormSchema(t)),
    [ t ],
  )

  const localPartRef = useInitialFocus()
  const form = useForm<CreateMailboxFormValues>({
    resolver,
    defaultValues: {
      localPart: '',
      // The default domain is preselected; with none, the first alphabetically.
      domainId: (props.domains.find((domain) => domain.isDefault) ?? props.domains[0])?.id ?? '',
      displayName: '',
    },
  })
  // What is typed is kept for the tab until the mailbox is created.
  const forgetDraft = useFormDraft(form, { name: 'mailbox', fields: [ 'localPart', 'displayName' ]})

  const save = saveForm(form, async (values) => {
    try {
      const response = await createMailbox(values)
      dispatch(mailAccountUpserted(response.account))
      forgetDraft()
      toast.success(t('toasts.created', { address: response.account.address }))
      void navigate(UrlTree.settingsEmail)
      return true
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 409) {
        form.setError('localPart', { message: t('createForm.errors.addressTaken') })
        return false
      }

      const message = getUpstreamErrorMessage(error)
      if (!message) {
        console.debug('CreateMailboxForm failed to create the mailbox', { error })
      }
      toast.danger(message ?? t('common:errors.unexpected'))
      return false
    }
  })

  const onSubmit = usePageSave({
    save,
    onDiscard: forgetDraft,
    isDirty: form.formState.isDirty,
    label: t('common:actions.create'),
  })

  const errors = form.formState.errors
  // useWatch subscribes per field and, unlike form.watch, is safe for the React Compiler.
  const [ localPart, domainId, displayName ] = useWatch({
    control: form.control,
    name: [ 'localPart', 'domainId', 'displayName' ],
  })
  const domainName = props.domains.find((domain) => domain.id === domainId)?.name
  const address = `${localPart.trim() || '…'}@${domainName ?? '…'}`.toLowerCase()

  return <Form onSubmit={onSubmit} validationBehavior='aria' className='flex max-w-xl flex-col gap-4'>
    <div className='grid grid-cols-1 gap-4 sm:grid-cols-2'>
      {/* Local part */}
      <TextField
        id='mailbox-local-part'
        isRequired
        autoComplete='off'
        isInvalid={Boolean(errors.localPart)}
        value={localPart}
        onChange={(value) => form.setValue('localPart', value, { shouldDirty: true, shouldValidate: true })}
      >
        <Label>{t('createForm.localPart')}</Label>
        <Input ref={localPartRef} placeholder='agent' />
        <FieldError>{errors.localPart?.message}</FieldError>
      </TextField>

      {/* Domain */}
      <Select
        id='mailbox-domain'
        isRequired
        isInvalid={Boolean(errors.domainId)}
        value={domainId || null}
        onChange={(key) => form.setValue(
          'domainId',
          String(key ?? ''),
          { shouldDirty: true, shouldValidate: true },
        )}
      >
        <Label>{t('createForm.domain')}</Label>
        <Select.Trigger>
          <Select.Value />
          <Select.Indicator />
        </Select.Trigger>
        <Select.Popover>
          <ListBox>{
            props.domains.map((domain) => <ListBox.Item
              key={domain.id}
              id={domain.id}
              textValue={domain.name}
            >
              {domain.name}
              <ListBox.ItemIndicator />
            </ListBox.Item>)
          }</ListBox>
        </Select.Popover>
        <FieldError>{errors.domainId?.message}</FieldError>
      </Select>
    </div>
    <div className='-mt-2 text-xs'>
      <p className='opacity-70'>{
        t('createForm.domainHint')
      }</p>
      <p className='mt-1 font-medium'>{
        t('createForm.addressPreview', { address })
      }</p>
    </div>

    {/* Sender name */}
    <TextField
      id='mailbox-display-name'
      autoComplete='off'
      isInvalid={Boolean(errors.displayName)}
      value={displayName}
      onChange={(value) => form.setValue('displayName', value, { shouldDirty: true, shouldValidate: true })}
    >
      <Label>{t('createForm.displayName')}</Label>
      <Input />
      <FieldError>{errors.displayName?.message}</FieldError>
    </TextField>

    <PageCreateFooter />
  </Form>
}
