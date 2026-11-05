// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, ComboBox, Input, Label, ListBox, Tooltip } from '@heroui/react'
import { LuX } from 'react-icons/lu'

type Props = {
  // The input's id, stable so the field can be found and labelled.
  id: string
  label: string
  // The labels to choose from, which can run long, so typing narrows them.
  labels: string[]
  value: string | null
  onChange: (label: string | null) => void
  isLabelHidden?: boolean
}

// One label to filter by, or none: picked from the labels in use, and cleared with the X beside it.
// A label that went out of use while it was chosen stays offered, so the field still names what is
// on.
export function LabelSelect(props: Props) {
  const { t } = useTranslation('knowledge')
  const options = props.value && !props.labels.includes(props.value)
    ? [ props.value, ...props.labels ]
    : props.labels

  return <div className='flex items-end gap-1'>
    <ComboBox
      id={props.id}
      className='min-w-0 flex-1'
      menuTrigger='focus'
      selectedKey={props.value}
      onSelectionChange={(key) => props.onChange(key === null
        ? null
        : String(key))}
    >
      <Label className={props.isLabelHidden
        ? 'sr-only'
        : undefined}>{props.label}</Label>
      <ComboBox.InputGroup>
        <Input
          autoComplete='off'
          placeholder={props.isLabelHidden
            ? props.label
            : undefined}
        />
        <ComboBox.Trigger />
      </ComboBox.InputGroup>
      <ComboBox.Popover>
        <ListBox>{
          options.map((label) => <ListBox.Item key={label} id={label} textValue={label}>
            {label}
            <ListBox.ItemIndicator />
          </ListBox.Item>)
        }</ListBox>
      </ComboBox.Popover>
    </ComboBox>
    {props.value && <Tooltip delay={400}>
      <Button
        isIconOnly
        variant='ghost'
        className='shrink-0'
        aria-label={t('labels.clearFilter')}
        onPress={() => props.onChange(null)}
      >
        <LuX className='size-4' aria-hidden />
      </Button>
      <Tooltip.Content>{t('labels.clearFilter')}</Tooltip.Content>
    </Tooltip>}
  </div>
}
