// Copyright © 2026 Jalapeno Labs

import type { ParseKeys, TFunction } from 'i18next'
import type { KnowledgePassage } from '../../api/routes/knowledgeRoutes'
import type { IconType } from 'react-icons'

// User interface
import { LuBuilding2, LuFileText, LuLightbulb, LuSend, LuVideo } from 'react-icons/lu'

// Misc
import { connectorLogos } from '../Settings/Connectors/connectorLogos'

// Every source the index holds documents of today, as `knowledge_documents.source` names them
// (docs/knowledge.md, What is indexed), in the order the filters offer them: mail, chat,
// pages and issues, meetings, then what lives in Elysium.
export const KNOWLEDGE_SOURCES = [
  'outlook',
  'gmail',
  'mail',
  'slack',
  'teams',
  'confluence',
  'youtrack',
  'meeting',
  'learning',
  'company_doc',
] as const
export type KnowledgeSource = typeof KNOWLEDGE_SOURCES[number]

type SourcePresentation = {
  labelKey: ParseKeys<'knowledge'>
  icon: IconType
  // A brand's own color for a connector's logo; Elysium's own sources follow the theme.
  className: string
}

const presentationBySource = {
  outlook: { labelKey: 'sources.outlook', ...connectorLogos.outlook },
  gmail: { labelKey: 'sources.gmail', ...connectorLogos.gmail },
  mail: { labelKey: 'sources.mail', icon: LuSend, className: '' },
  slack: { labelKey: 'sources.slack', ...connectorLogos.slack },
  teams: { labelKey: 'sources.teams', ...connectorLogos.teams },
  confluence: { labelKey: 'sources.confluence', ...connectorLogos.confluence },
  youtrack: { labelKey: 'sources.youtrack', ...connectorLogos.youtrack },
  meeting: { labelKey: 'sources.meeting', icon: LuVideo, className: '' },
  learning: { labelKey: 'sources.learning', icon: LuLightbulb, className: '' },
  company_doc: { labelKey: 'sources.company_doc', icon: LuBuilding2, className: '' },
} as const satisfies Record<KnowledgeSource, SourcePresentation>

// A passage's text as a page shows it under its document's title. A document's text begins
// with its title (docs/knowledge.md, Timeline records), which the page already shows above it,
// so the first passage leaves it out; every other passage is shown as it is.
export function withoutTitle(passage: KnowledgePassage, title: string) {
  const leading = `${title}\n\n`
  if (passage.position !== 0 || !title || !passage.content.startsWith(leading)) {
    return passage.content
  }
  return passage.content.slice(leading.length)
}

function isKnowledgeSource(source: string): source is KnowledgeSource {
  return Object.hasOwn(presentationBySource, source)
}

// A source's name and icon. One this build does not know yet, which a newer API may index,
// reads as its own name with a plain document's icon.
export function presentKnowledgeSource(source: string, t: TFunction<'knowledge'>) {
  if (!isKnowledgeSource(source)) {
    console.debug('presentKnowledgeSource met a source it does not know', { source })
    return { label: source, icon: LuFileText, className: '' } as const
  }
  const presentation = presentationBySource[source]
  return {
    label: t(presentation.labelKey),
    icon: presentation.icon,
    className: presentation.className,
  } as const
}
