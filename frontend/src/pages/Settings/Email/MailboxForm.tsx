// Copyright © 2026 Jalapeno Labs

import type { MailAccount } from '../../../api/routes/mailRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { mailAccountUpserted } from '../../../store/mailAccountsSlice'

// User interface
import { Description, Form, Input, Label, Switch, TextField } from '@heroui/react'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { updateMailAccount } from '../../../api/routes/mailRoutes'
import { useInitialFocus } from '../../../hooks/useInitialFocus'
import { usePageSave } from '../../../layout/page/pageSave'
import { DISPLAY_NAME_MAX_CHARACTERS } from './mailFormSchemas'
import { toast } from '../../../toast/toast'

type Props = {
  account: MailAccount
  onSaved: () => void
}

type MailboxChanges = {
  displayName?: string
  isActive?: boolean
}

// What can change about a mailbox: the name recipients see beside its address, and whether
// Elysium reads and sends from it. Saved from the page's Save. Only what is changed here is
// held and sent, so a change made elsewhere to the other field is never undone.
export function MailboxForm(props: Props) {
  const { t } = useTranslation([ 'email', 'common' ])
  const dispatch = useAppDispatch()
  const nameRef = useInitialFocus()
  const [ changes, setChanges ] = useState<MailboxChanges>({})
  const account = props.account
  const displayName = changes.displayName ?? account.displayName
  const isActive = changes.isActive ?? account.isActive
  const isDirty = displayName.trim() !== account.displayName || isActive !== account.isActive

  async function save() {
    try {
      const response = await updateMailAccount(account.id, {
        ...changes,
        displayName: changes.displayName?.trim(),
      })
      dispatch(mailAccountUpserted(response.account))
      setChanges({})
      toast.success(t('toasts.updated', { address: response.account.address }))
      props.onSaved()
      return true
    }
    catch (error) {
      console.debug('MailboxForm failed to save the mailbox', { error, accountId: account.id })
      toast.danger(t('common:errors.unexpected'), { description: getApiErrorMessage(error) })
      return false
    }
  }

  const onSubmit = usePageSave({ save, isDirty })

  return <Form onSubmit={onSubmit} className='flex max-w-xl flex-col gap-6'>
    <TextField
      id='mailbox-display-name'
      value={displayName}
      maxLength={DISPLAY_NAME_MAX_CHARACTERS}
      autoComplete='off'
      onChange={(value) => setChanges({ ...changes, displayName: value })}
    >
      <Label>{t('mailboxForm.displayName')}</Label>
      <Input ref={nameRef} />
      <Description>{t('mailboxForm.displayNameHint', { address: account.address })}</Description>
    </TextField>

    <div>
      <Switch
        id='mailbox-active'
        isSelected={isActive}
        onChange={(isSelected) => setChanges({ ...changes, isActive: isSelected })}
      >
        <Switch.Content>
          <Switch.Control>
            <Switch.Thumb />
          </Switch.Control>
          <Label>{t('mailboxForm.isActive')}</Label>
        </Switch.Content>
      </Switch>
      <Description className='mt-1 block'>{t('mailboxForm.isActiveHint')}</Description>
    </div>
  </Form>
}
