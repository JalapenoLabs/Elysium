// Copyright © 2026 Jalapeno Labs

// Core
import i18next from 'i18next'
import { initReactI18next } from 'react-i18next'

// Misc
import { formatNumber } from './format/numbers'
import actionItems from './locales/en-US/actionItems.json'
import auth from './locales/en-US/auth.json'
import backups from './locales/en-US/backups.json'
import browsers from './locales/en-US/browsers.json'
import changesets from './locales/en-US/changesets.json'
import coding from './locales/en-US/coding.json'
import common from './locales/en-US/common.json'
import companyDoc from './locales/en-US/companyDoc.json'
import connectors from './locales/en-US/connectors.json'
import elysia from './locales/en-US/elysia.json'
import email from './locales/en-US/email.json'
import environment from './locales/en-US/environment.json'
import github from './locales/en-US/github.json'
import initiatives from './locales/en-US/initiatives.json'
import issueReports from './locales/en-US/issueReports.json'
import jira from './locales/en-US/jira.json'
import learningCategories from './locales/en-US/learningCategories.json'
import learnings from './locales/en-US/learnings.json'
import llms from './locales/en-US/llms.json'
import meetings from './locales/en-US/meetings.json'
import models from './locales/en-US/models.json'
import navigation from './locales/en-US/navigation.json'
import oauth from './locales/en-US/oauth.json'
import passwords from './locales/en-US/passwords.json'
import projects from './locales/en-US/projects.json'
import runs from './locales/en-US/runs.json'
import search from './locales/en-US/search.json'
import settings from './locales/en-US/settings.json'
import storage from './locales/en-US/storage.json'
import studio from './locales/en-US/studio.json'
import timeline from './locales/en-US/timeline.json'
import updates from './locales/en-US/updates.json'
import users from './locales/en-US/users.json'

export const DEFAULT_LOCALE = 'en-US'
export const DEFAULT_NAMESPACE = 'common'

// en-US is the source locale and, today, the only one shipped. Bundling it keeps
// the first render synchronous; other locales can load on demand once adopted.
export const resources = {
  [DEFAULT_LOCALE]: {
    actionItems,
    auth,
    backups,
    browsers,
    changesets,
    coding,
    common,
    companyDoc,
    elysia,
    email,
    environment,
    connectors,
    github,
    initiatives,
    issueReports,
    jira,
    learningCategories,
    learnings,
    llms,
    meetings,
    models,
    navigation,
    oauth,
    passwords,
    projects,
    runs,
    search,
    settings,
    storage,
    studio,
    timeline,
    updates,
    users,
  },
} as const

// Every namespace, for code that reads keys from all of them, such as search's registry,
// which reuses each area's own words.
export type TranslationNamespace = keyof (typeof resources)[typeof DEFAULT_LOCALE]
// `Object.keys` only knows it returns strings; these are exactly the bundled namespaces.
export const TRANSLATION_NAMESPACES = Object.keys(resources[DEFAULT_LOCALE]) as TranslationNamespace[]

await i18next
  .use(initReactI18next)
  .init({
    lng: DEFAULT_LOCALE,
    fallbackLng: DEFAULT_LOCALE,
    defaultNS: DEFAULT_NAMESPACE,
    ns: TRANSLATION_NAMESPACES,
    resources,
    interpolation: {
      // React already escapes rendered strings.
      escapeValue: false,
    },
  })

// A count in a translation compresses like every other number: `{{count, compact}}` reads
// "1.4M records", while `count` itself stays a number for choosing the plural form.
if (i18next.services.formatter) {
  i18next.services.formatter.add('compact', (value: number) => formatNumber(value))
}
else {
  console.warn('i18next started without its formatter, so counts in translations are not compressed')
}
