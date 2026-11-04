// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../../search/searchTypes'

// User interface
import {
  LuFingerprint,
  LuKey,
  LuLock,
  LuMonitorSmartphone,
  LuPencil,
  LuShieldCheck,
  LuShieldPlus,
  LuSmartphone,
  LuTrash2,
} from 'react-icons/lu'

// Misc
import { UrlTree, getSecurityTabUrl } from '../../../urls'

// Security, and each thing on it, so "2FA" or "passkey" lands where it is kept, and the
// two-factor pages that apply: Add while it is off, Change and Remove while it is on, as its
// card offers them. See src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'security',
    group: 'pages',
    title: 'settings:items.security.title',
    tags: 'search:tags.security',
    parent: 'settings:title',
    icon: LuShieldCheck,
    href: UrlTree.settingsSecurity,
  },
  {
    id: 'password',
    group: 'pages',
    title: 'auth:security.password.title',
    tags: 'search:tags.password',
    parent: 'auth:security.title',
    icon: LuLock,
    href: UrlTree.settingsSecurity,
  },
  {
    id: 'twoFactor',
    group: 'pages',
    title: 'auth:security.twoFactor.title',
    tags: 'search:tags.twoFactor',
    parent: 'auth:security.title',
    icon: LuSmartphone,
    href: UrlTree.settingsSecurity,
  },
  {
    id: 'passkeys',
    group: 'pages',
    title: 'auth:security.tabs.passkeys',
    tags: 'search:tags.passkeys',
    parent: 'auth:security.title',
    icon: LuFingerprint,
    href: getSecurityTabUrl('passkeys'),
  },
  {
    id: 'lookupCodes',
    group: 'pages',
    title: 'auth:security.tabs.lookup',
    tags: 'search:tags.lookupCodes',
    parent: 'auth:security.title',
    icon: LuKey,
    href: getSecurityTabUrl('lookup'),
  },
  {
    id: 'sessions',
    group: 'pages',
    title: 'auth:security.tabs.sessions',
    tags: 'search:tags.sessions',
    parent: 'auth:security.title',
    icon: LuMonitorSmartphone,
    href: getSecurityTabUrl('sessions'),
  },
  {
    id: 'addTwoFactor',
    group: 'actions',
    title: 'auth:security.twoFactor.add',
    tags: 'search:tags.addTwoFactor',
    parent: 'auth:security.twoFactor.title',
    icon: LuShieldPlus,
    href: UrlTree.settingsTwoFactorNew,
    when: 'twoFactorOff',
  },
  {
    id: 'changeTwoFactor',
    group: 'actions',
    title: 'auth:security.twoFactor.change',
    tags: 'search:tags.changeTwoFactor',
    parent: 'auth:security.twoFactor.title',
    icon: LuPencil,
    href: UrlTree.settingsTwoFactorEdit,
    when: 'twoFactorOn',
  },
  {
    id: 'removeTwoFactor',
    group: 'actions',
    title: 'auth:security.twoFactor.remove',
    tags: 'search:tags.removeTwoFactor',
    parent: 'auth:security.twoFactor.title',
    icon: LuTrash2,
    href: UrlTree.settingsTwoFactorRemove,
    when: 'twoFactorOn',
  },
]
