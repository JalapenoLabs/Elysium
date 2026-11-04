// Copyright © 2026 Jalapeno Labs

import type { RefCallback } from 'react'

// Core
import { useCallback } from 'react'

// Focuses the element it is given as soon as it is in the document, so typing starts where
// the page or dialog expects without a click (`~/.claude/docs/ux.md`, Inputs). Pass the ref to
// the input, text area, or button itself:
//
//   const titleRef = useInitialFocus()
//   <Input ref={titleRef} />
//
// A ref callback rather than the `autoFocus` attribute or a ref read in a mount effect. The
// attribute is unreliable when a form animates in and under StrictMode's double mount
// (`react.md`). A mount effect runs once, while many fields only exist while something is open:
// a dialog's body inside an always-mounted gate, a picker's search inside a popover. A ref
// callback runs each time its element attaches, whenever that is. It runs before React Aria's
// dialogs and popovers move focus to themselves, and they leave focus alone once it is inside.
//
// `shouldFocus` picks the field when the choice depends on the page, such as the prompt rather
// than the title when the title is already filled in. The ref takes any element, so one hook
// suits an input, a text area, or a button.
export function useInitialFocus(shouldFocus = true): RefCallback<HTMLElement> {
  return useCallback((element: HTMLElement | null) => {
    if (shouldFocus) {
      element?.focus()
    }
  }, [ shouldFocus ])
}
