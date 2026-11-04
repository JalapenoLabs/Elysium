// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'
import type { PageAction } from './pageActions'

// User interface
import { PageActionButton } from './PageActionButton'
import { PageActionsMenu } from './PageActionsMenu'

// Misc
import { arrangePageActions, isAnyPageActionPending } from './pageActions'

type Props = {
  actions: PageAction[]
  // The page's Save. It follows a lone button, so it keeps the top right corner, but comes
  // before the Actions menu, which is always furthest right.
  save?: ReactNode
}

// The right end of a page's titlebar: a lone harmless action as its own button, anything
// more as one Actions menu with destructive items last, and the page's Save between them.
// See `arrangePageActions` for the rule.
export function PageActions(props: Props) {
  const { button, regular, dangerous } = arrangePageActions(props.actions)
  const hasMenu = Boolean(regular.length || dangerous.length)

  return <>
    {button && <PageActionButton action={button} />}
    {props.save}
    {hasMenu && <PageActionsMenu
      regular={regular}
      dangerous={dangerous}
      isPending={isAnyPageActionPending(props.actions)}
    />}
  </>
}
