// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { KeyboardEvent } from 'react'
import type { KnowledgePage } from '../../../api/routes/pageRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import { pageLoaded, selectLabelsInUse } from '../../../store/pagesSlice'

// User interface
import { Button, ComboBox, EmptyState, Input, Label, ListBox, Tooltip } from '@heroui/react'
import { LuX } from 'react-icons/lu'
import { LabelLink } from './LabelLink'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { addPageLabel, removePageLabel } from '../../../api/routes/pageRoutes'
import { toast } from '../../../toast/toast'

type Props = {
  page: KnowledgePage
}

// A page's labels, each opening the pages that carry it, and the field that adds one: picked from
// the labels in use or typed. A label is an action, not a field of the page, so each applies at once
// and makes no version; taking one off is undone from its toast. A deleted page changes no further,
// so it only reads them.
export function PageLabels(props: Props) {
  const { page } = props
  const { t } = useTranslation([ 'knowledge', 'common' ])
  const dispatch = useAppDispatch()
  const labelsInUse = useAppSelector(selectLabelsInUse)
  const [ draft, setDraft ] = useState('')
  const [ isAdding, setIsAdding ] = useState(false)
  const [ removingLabel, setRemovingLabel ] = useState<string | null>(null)

  const isReadOnly = Boolean(page.deletedAt)
  const suggestions = labelsInUse.filter((label) => !page.labels.includes(label))

  async function add(label: string) {
    setIsAdding(true)
    try {
      const response = await addPageLabel(page.id, label)
      dispatch(pageLoaded(response.page))
      setDraft('')
    }
    catch (error) {
      console.debug('PageLabels failed to add a label', { error, pageId: page.id, label })
      toast.danger(t('labels.addFailed'), { description: getApiErrorMessage(error) })
    }
    finally {
      setIsAdding(false)
    }
  }

  async function remove(label: string) {
    setRemovingLabel(label)
    try {
      const response = await removePageLabel(page.id, label)
      dispatch(pageLoaded(response.page))
      toast.success(t('labels.removed', { label }), {
        action: {
          label: t('common:actions.undo'),
          onPress: () => void add(label),
        },
      })
    }
    catch (error) {
      console.debug('PageLabels failed to remove a label', { error, pageId: page.id, label })
      toast.danger(t('labels.removeFailed'), { description: getApiErrorMessage(error) })
    }
    finally {
      setRemovingLabel(null)
    }
  }

  // A highlighted suggestion is the combobox's to pick, which it marks on the input. Otherwise
  // Enter adds what was typed, which the combobox would only close over.
  function addTypedLabel(event: KeyboardEvent<HTMLDivElement>) {
    const input = event.target
    const isSuggestionHighlighted = input instanceof Element && input.hasAttribute('aria-activedescendant')
    if (event.key !== 'Enter' || isSuggestionHighlighted || !draft.trim()) {
      return
    }
    event.preventDefault()
    event.stopPropagation()
    void add(draft)
  }

  if (isReadOnly && !page.labels.length) {
    return null
  }

  return <div className='flex flex-wrap items-center gap-1.5'>
    {Boolean(page.labels.length) && <ul aria-label={t('labels.label')} className='contents'>{
      page.labels.map((label) => <li key={label} className='flex items-center'>
        <LabelLink label={label} />
        {!isReadOnly && <Tooltip delay={400}>
          <Button
            isIconOnly
            size='sm'
            variant='ghost'
            className='size-5 min-w-0'
            aria-label={t('labels.remove', { label })}
            isPending={removingLabel === label}
            onPress={() => void remove(label)}
          >
            <LuX className='size-3' aria-hidden />
          </Button>
          <Tooltip.Content>{t('labels.remove', { label })}</Tooltip.Content>
        </Tooltip>}
      </li>)
    }</ul>}
    {!isReadOnly && <div onKeyDownCapture={addTypedLabel}>
      <ComboBox
        id='page-label-add'
        className='w-40'
        allowsCustomValue
        menuTrigger='focus'
        isReadOnly={isAdding}
        inputValue={draft}
        onInputChange={setDraft}
        selectedKey={null}
        onSelectionChange={(key: Key | null) => {
          if (key !== null) {
            void add(String(key))
          }
        }}
      >
        <Label className='sr-only'>{t('labels.add')}</Label>
        <ComboBox.InputGroup>
          <Input autoComplete='off' className='h-8 text-sm' placeholder={t('labels.add')} />
          <ComboBox.Trigger />
        </ComboBox.InputGroup>
        <ComboBox.Popover>
          <ListBox renderEmptyState={() => <EmptyState />}>{
            suggestions.map((label) => <ListBox.Item key={label} id={label} textValue={label}>
              {label}
            </ListBox.Item>)
          }</ListBox>
        </ComboBox.Popover>
      </ComboBox>
    </div>}
  </div>
}
