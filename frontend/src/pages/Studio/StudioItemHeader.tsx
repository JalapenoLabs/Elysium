// Copyright © 2026 Jalapeno Labs

import type { StudioItem } from '../../api/routes/studioRoutes'
import type { StudioLayoutMode } from './studioLayout'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { PageHeader } from '../../layout/page/PageHeader'
import { StudioLayoutControl } from './StudioLayoutControl'

// Misc
import { UrlTree } from '../../urls'
import { useStudioItemActions } from './useStudioItemActions'

type Props = {
  item: StudioItem
  layoutMode: StudioLayoutMode
  onLayoutModeChange: (mode: StudioLayoutMode) => void
}

// The item page's top bar, the standard page header: back to the grid, the title, which
// columns show, and the item's actions, Rename among them.
export function StudioItemHeader(props: Props) {
  const { t } = useTranslation('studio')
  const actions = useStudioItemActions(props.item)

  return <PageHeader
    className='shrink-0 border-b border-separator px-4 py-3'
    title={props.item.title}
    parents={[{ label: t('title'), href: UrlTree.studio }]}
    tools={<StudioLayoutControl
      mode={props.layoutMode}
      onChange={props.onLayoutModeChange}
    />}
    actions={actions}
  />
}
