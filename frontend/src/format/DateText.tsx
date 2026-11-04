// Copyright © 2026 Jalapeno Labs

import type { DateInput } from './dates'

// User interface
import { Tooltip } from '@heroui/react'

// Misc
import { formatDate, formatDateTime, formatShortDate } from './dates'

type Props = {
  value: DateInput
  // A day rather than a moment, such as a due date, whose time of day is only the day's end:
  // the tooltip leaves the time out.
  isDay?: boolean
  // Whether the trigger is a tab stop (the default), so the full timestamp reaches the keyboard.
  // Tables pass false: a page of rows would otherwise hold one tab stop per date between the
  // controls the keyboard is after, and the tooltip still shows on hover.
  isFocusable?: boolean
  // Styles the trigger, the element that sits in the surrounding layout.
  className?: string
}

// A date in its short form, "January 1st", for tables and lists, with the full timestamp in a
// tooltip. It renders a `div`, so it belongs in a `div` or `span` parent, never a `p`.
export function DateText(props: Props) {
  const date = new Date(props.value)

  // HeroUI makes every trigger a focusable button. Out of the tab order it is no control at all,
  // so it says so rather than announcing a button the keyboard cannot reach. Left out entirely
  // when focusable: HeroUI spreads these over its own, so even an undefined role would erase
  // its button role.
  const outOfTabOrder = props.isFocusable === false
    ? { tabIndex: -1, role: 'presentation' }
    : {}

  return <Tooltip delay={400}>
    <Tooltip.Trigger
      className={`inline ${props.className ?? ''}`}
      {...outOfTabOrder}
    >
      <time
        dateTime={Number.isNaN(date.getTime())
          ? undefined
          : date.toISOString()}
      >{
          formatShortDate(props.value)
        }</time>
    </Tooltip.Trigger>
    <Tooltip.Content>{
      props.isDay
        ? formatDate(props.value)
        : formatDateTime(props.value)
    }</Tooltip.Content>
  </Tooltip>
}
