// Copyright © 2026 Jalapeno Labs

// Core
import i18next from 'i18next'
import { initReactI18next } from 'react-i18next'

// Misc
import actionItems from './locales/en-US/actionItems.json'
import changesets from './locales/en-US/changesets.json'
import coding from './locales/en-US/coding.json'
import common from './locales/en-US/common.json'
import email from './locales/en-US/email.json'
import environment from './locales/en-US/environment.json'
import github from './locales/en-US/github.json'
import initiatives from './locales/en-US/initiatives.json'
import jira from './locales/en-US/jira.json'
import llms from './locales/en-US/llms.json'
import navigation from './locales/en-US/navigation.json'
import projects from './locales/en-US/projects.json'
import satellites from './locales/en-US/satellites.json'
import settings from './locales/en-US/settings.json'
import storage from './locales/en-US/storage.json'
import studio from './locales/en-US/studio.json'

export const DEFAULT_LOCALE = 'en-US'
export const DEFAULT_NAMESPACE = 'common'

// en-US is the source locale and, today, the only one shipped. Bundling it keeps
// the first render synchronous; other locales can load on demand once adopted.
export const resources = {
  [DEFAULT_LOCALE]: {
    actionItems,
    changesets,
    coding,
    common,
    email,
    environment,
    github,
    initiatives,
    jira,
    llms,
    navigation,
    projects,
    satellites,
    settings,
    storage,
    studio,
  },
} as const

await i18next
  .use(initReactI18next)
  .init({
    lng: DEFAULT_LOCALE,
    fallbackLng: DEFAULT_LOCALE,
    defaultNS: DEFAULT_NAMESPACE,
    ns: Object.keys(resources[DEFAULT_LOCALE]),
    resources,
    interpolation: {
      // React already escapes rendered strings.
      escapeValue: false,
    },
  })
