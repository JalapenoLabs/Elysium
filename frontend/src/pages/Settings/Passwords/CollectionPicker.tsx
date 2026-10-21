// Copyright © 2026 Jalapeno Labs

import type { PasswordVault, VaultStatus } from '../../../api/routes/passwordVaultRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { passwordVaultUpdated } from '../../../store/passwordVaultSlice'

// User interface
import { Description, Label, ListBox, Select, Spinner, toast } from '@heroui/react'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { chooseVaultCollection, listVaultCollections } from '../../../api/routes/passwordVaultRoutes'

type Props = {
  vault: PasswordVault
  status: VaultStatus
}

// The id the "none" option carries; no Bitwarden collection id looks like it.
const NO_COLLECTION = 'none'

// Chooses the one collection agents may read, from those the account reaches as the CLI
// answers them, so no id is ever typed.
export function CollectionPicker(props: Props) {
  const { t } = useTranslation('passwords')
  const dispatch = useAppDispatch()
  const [ isSaving, setIsSaving ] = useState(false)
  const isReady = props.status.state === 'ready'
  // A form's one-off read, so it stays in SWR rather than Redux. Keyed by the account, so a
  // vault connected as someone else lists that account's collections.
  const { data, error } = useSWR(
    isReady
      ? [ 'v1/password-vault/collections', props.vault.accountEmail ]
      : null,
    () => listVaultCollections(),
  )

  async function choose(collectionId: string | null) {
    setIsSaving(true)
    try {
      const response = await chooseVaultCollection(collectionId)
      if (response.vault) {
        dispatch(passwordVaultUpdated(response.vault))
      }
      toast.success(
        response.vault?.collection
          ? t('toasts.collectionChosen', { name: response.vault.collection.name })
          : t('toasts.collectionCleared'),
      )
    }
    catch (chooseError) {
      console.debug('CollectionPicker could not choose the collection', { chooseError })
      toast.danger(t('toasts.failed'), { description: getApiErrorMessage(chooseError) ?? undefined })
    }
    setIsSaving(false)
  }

  let body = null
  if (!isReady) {
    body = <p className='text-sm opacity-70'>{t('collection.notReady')}</p>
  }
  else if (error) {
    body = <p className='text-sm text-danger'>{t('collection.loadError')}</p>
  }
  else if (!data) {
    body = <Spinner size='sm' />
  }
  else {
    body = <Select
      className='max-w-md'
      isDisabled={isSaving}
      value={props.vault.collection?.id ?? NO_COLLECTION}
      onChange={(key) => {
        const chosen = data.collections.find((collection) => collection.id === key)
        if (!chosen && key !== NO_COLLECTION) {
          console.debug('CollectionPicker ignored an unknown collection', { key })
          return
        }
        void choose(chosen?.id ?? null)
      }}
    >
      <Label>{t('collection.label')}</Label>
      <Select.Trigger>
        <Select.Value />
        <Select.Indicator />
      </Select.Trigger>
      <Select.Popover>
        <ListBox>
          <ListBox.Item id={NO_COLLECTION} textValue={t('collection.none')}>
            {t('collection.none')}
            <ListBox.ItemIndicator />
          </ListBox.Item>
          {data.collections.map((collection) => <ListBox.Item
            key={collection.id}
            id={collection.id}
            textValue={collection.name}
          >
            {collection.name}
            <ListBox.ItemIndicator />
          </ListBox.Item>)}
        </ListBox>
      </Select.Popover>
      <Description>{t('collection.description')}</Description>
    </Select>
  }

  return <section className='rounded-xl border border-separator p-6'>
    <h2 className='compact text-xl font-semibold'>{t('collection.heading')}</h2>
    {body}
  </section>
}
