// Copyright © 2026 Jalapeno Labs

import type { ParseKeys, TFunction } from 'i18next'
import type { PageAttachment, PageAttachmentPolicy } from '../../../api/routes/pageRoutes'

// Misc
import { getApiErrorCode, getApiErrorMessage } from '../../../api/errors'
import { formatBytes } from '../../../format/numbers'

// Why a file was refused, for what the API refuses by a stable code.
const REFUSAL_KEYS = {
  attachment_too_large: 'attachments.refusals.tooLarge',
  attachment_empty: 'attachments.refusals.empty',
  attachment_name: 'attachments.refusals.unnamed',
  attachment_not_an_image: 'attachments.refusals.notAnImage',
  attachment_no_location: 'attachments.refusals.noLocation',
  page_attachment_limit: 'attachments.refusals.limit',
  page_deleted: 'attachments.refusals.pageDeleted',
  page_proposal_only: 'attachments.refusals.proposalOnly',
} as const satisfies Record<string, ParseKeys<'knowledge'>>

const refusalKeys: Record<string, typeof REFUSAL_KEYS[keyof typeof REFUSAL_KEYS] | undefined> = REFUSAL_KEYS

// The extension a name ends in, lowercase; empty for a name without one.
function extensionOf(name: string) {
  const dot = name.lastIndexOf('.')
  return dot < 0
    ? ''
    : name.slice(dot + 1).toLowerCase()
}

// Why a page will not take `file`, said before anyone waits for it to upload; null when it will.
// The API has the last word on all of this: these are its limits, read from its policy.
export function describeFileRefusal(
  file: Pick<File, 'name' | 'size'>,
  policy: PageAttachmentPolicy,
  t: TFunction<'knowledge'>,
) {
  const extension = extensionOf(file.name)
  if (!extension) {
    return t('attachments.refusals.unnamed')
  }
  if (!policy.extensions.includes(extension)) {
    return t('attachments.refusals.type', { extension })
  }
  if (!file.size) {
    return t('attachments.refusals.empty')
  }
  if (file.size > policy.maxBytes) {
    return t('attachments.refusals.tooLarge', { size: formatBytes(policy.maxBytes) })
  }
  return null
}

// What the API's refusal of a file means: the app's words where it names one, and otherwise its
// own, which for a storage provider's refusal are what to fix.
export function describeUploadRefusal(
  error: unknown,
  policy: PageAttachmentPolicy,
  t: TFunction<'knowledge'>,
) {
  const key = refusalKeys[getApiErrorCode(error) ?? '']
  if (key) {
    return t(key, { size: formatBytes(policy.maxBytes) })
  }
  return getApiErrorMessage(error) ?? t('attachments.uploadStopped')
}

// What a file input offers, so the picker shows what a page takes.
export function attachmentAccept(policy: PageAttachmentPolicy) {
  return policy.extensions
    .map((extension) => `.${extension}`)
    .join(',')
}

// How a page's Markdown refers to a file: an image displays, and anything else is a link.
export function attachmentMarkdown(attachment: Pick<PageAttachment, 'name' | 'url' | 'isImage'>) {
  const name = attachment.name.replace(/[[\]\\]/g, '\\$&')
  if (attachment.isImage) {
    return `![${name}](${attachment.url})`
  }
  return `[${name}](${attachment.url})`
}
