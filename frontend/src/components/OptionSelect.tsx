// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { PickerOption } from './MultiPicker'

// User interface
import { Label, ListBox, Select } from '@heroui/react'

type Props = {
  // The trigger's id, stable so the field can be found and labelled.
  id: string
  label: string
  options: PickerOption[]
  value: string
  onChange: (value: string) => void
  className?: string
  isDisabled?: boolean
  // Shown while nothing is chosen, for a select that starts empty.
  placeholder?: string
  // For a select whose purpose its surroundings already say, such as a table cell: the
  // label is still read by screen readers.
  isLabelHidden?: boolean
}

// A labelled select over a short, fixed list, such as a filter's Any, Yes, and No, or an
// item's priority.
export function OptionSelect(props: Props) {
  return <Select
    id={props.id}
    className={props.className}
    isDisabled={props.isDisabled}
    placeholder={props.placeholder}
    value={props.value}
    onChange={(key: Key | null) => {
      if (key === null) {
        console.debug('OptionSelect ignored an empty selection', { label: props.label })
        return
      }
      props.onChange(String(key))
    }}
  >
    <Label className={props.isLabelHidden
      ? 'sr-only'
      : undefined}>{props.label}</Label>
    <Select.Trigger>
      <Select.Value />
      <Select.Indicator />
    </Select.Trigger>
    <Select.Popover>
      <ListBox>{
        props.options.map((option) => <ListBox.Item key={option.id} id={option.id} textValue={option.label}>
          {option.label}
          <ListBox.ItemIndicator />
        </ListBox.Item>)
      }</ListBox>
    </Select.Popover>
  </Select>
}
