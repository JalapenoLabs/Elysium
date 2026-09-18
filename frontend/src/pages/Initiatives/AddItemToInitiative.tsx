// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { Initiative } from '../../api/routes/initiativeRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch, useAppSelector } from '../../store/hooks'
import { actionItemUpserted, selectLiveActionItems } from '../../store/actionItemsSlice'

// User interface
import { ComboBox, EmptyState, Input, Label, ListBox, toast } from '@heroui/react'

// Misc
import { joinInitiative } from '../../api/routes/actionItemRoutes'

type Props = {
  initiative: Initiative
}

// Finds an existing item by title and adds it to the initiative. The field clears after
// each pick, ready for the next.
export function AddItemToInitiative(props: Props) {
  const { t } = useTranslation([ 'initiatives', 'common' ])
  const dispatch = useAppDispatch()
  const items = useAppSelector(selectLiveActionItems)
  const [ query, setQuery ] = useState('')
  const [ isAdding, setIsAdding ] = useState(false)

  const candidates = items.filter((item) => !item.initiativeIds.includes(props.initiative.id))

  async function add(key: Key | null) {
    if (key === null) {
      return
    }
    const itemId = String(key)
    setIsAdding(true)
    try {
      const response = await joinInitiative(itemId, props.initiative.id)
      dispatch(actionItemUpserted(response.item))
      setQuery('')
    }
    catch (error) {
      console.debug('AddItemToInitiative failed to add an item', { error, itemId })
      toast.danger(t('common:errors.unexpected'))
    }
    finally {
      setIsAdding(false)
    }
  }

  return <ComboBox
    className='w-full max-w-md'
    isDisabled={isAdding}
    inputValue={query}
    onInputChange={setQuery}
    selectedKey={null}
    onSelectionChange={(key) => void add(key)}
  >
    <Label>{t('members.add')}</Label>
    <ComboBox.InputGroup>
      <Input placeholder={t('members.addPlaceholder')} />
      <ComboBox.Trigger />
    </ComboBox.InputGroup>
    <ComboBox.Popover>
      <ListBox renderEmptyState={() => <EmptyState>{t('members.noCandidates')}</EmptyState>}>{
        candidates.map((item) => <ListBox.Item key={item.id} id={item.id} textValue={item.title}>
          {item.title}
        </ListBox.Item>)
      }</ListBox>
    </ComboBox.Popover>
  </ComboBox>
}
