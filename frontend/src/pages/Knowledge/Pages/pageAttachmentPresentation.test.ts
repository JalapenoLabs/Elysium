// Copyright © 2026 Jalapeno Labs

import type { PageAttachmentPolicy } from '../../../api/routes/pageRoutes'

// Core
import i18next from 'i18next'
import { HTTPError } from 'ky'
import { describe, expect, it } from 'vitest'

// Misc
import {
  attachmentAccept,
  attachmentMarkdown,
  describeFileRefusal,
  describeUploadRefusal,
} from './pageAttachmentPresentation'

const t = i18next.getFixedT('en-US', 'knowledge')

const policy: PageAttachmentPolicy = {
  destinationId: 'location',
  maxBytes: 50_000_000,
  extensions: [ 'png', 'pdf' ],
}

function refused(data: unknown) {
  const error = new HTTPError(new Response(null, { status: 400 }), new Request('http://localhost/'), {} as never)
  error.data = data
  return error
}

describe('describeFileRefusal', () => {
  it('takes what the policy takes, however its extension is written', () => {
    expect(describeFileRefusal({ name: 'plan.png', size: 10 }, policy, t)).toBeNull()
    expect(describeFileRefusal({ name: 'PLAN.PDF', size: 10 }, policy, t)).toBeNull()
  })

  it('names a type a page does not take', () => {
    expect(describeFileRefusal({ name: 'tool.exe', size: 10 }, policy, t)).toBe('Pages don\'t take .exe files.')
  })

  it('says the limit when a file is over it', () => {
    expect(describeFileRefusal({ name: 'plan.pdf', size: 50_000_001 }, policy, t))
      .toBe('Files can be up to 50 MB.')
    expect(describeFileRefusal({ name: 'plan.pdf', size: 50_000_000 }, policy, t)).toBeNull()
  })

  it('refuses an empty file and a name with no extension', () => {
    expect(describeFileRefusal({ name: 'plan.pdf', size: 0 }, policy, t)).toBe('This file is empty.')
    expect(describeFileRefusal({ name: 'plan', size: 1 }, policy, t))
      .toBe('A file needs a name and an extension.')
  })
})

describe('describeUploadRefusal', () => {
  it('says who can fix a page with nowhere to keep files, and where', () => {
    const error = refused({ code: 'attachment_no_location', message: 'the API\'s words' })

    expect(describeUploadRefusal(error, policy, t))
      .toBe('An admin chooses where attachments are kept, in Settings, Storage.')
  })

  it('says the limit in the app\'s own words', () => {
    const error = refused({ code: 'attachment_too_large', message: 'the API\'s words' })

    expect(describeUploadRefusal(error, policy, t)).toBe('Files can be up to 50 MB.')
  })

  it('passes on what a storage provider said', () => {
    const error = refused({ message: 'the access key was refused' })

    expect(describeUploadRefusal(error, policy, t)).toBe('the access key was refused')
  })

  it('still says something when nothing answered', () => {
    expect(describeUploadRefusal(new TypeError('Failed to fetch'), policy, t))
      .toBe('The upload stopped before it finished. Try it again.')
  })
})

describe('attachmentAccept', () => {
  it('is what a file input takes', () => {
    expect(attachmentAccept(policy)).toBe('.png,.pdf')
  })
})

describe('attachmentMarkdown', () => {
  const url = '/api/v1/pages/one/attachments/two/plan.png'

  it('shows an image and links anything else', () => {
    expect(attachmentMarkdown({ name: 'plan.png', url, isImage: true })).toBe(`![plan.png](${url})`)
    expect(attachmentMarkdown({ name: 'plan.pdf', url, isImage: false })).toBe(`[plan.pdf](${url})`)
  })

  it('keeps a name from closing the text it sits in', () => {
    expect(attachmentMarkdown({ name: 'a]b[c.pdf', url, isImage: false })).toBe(`[a\\]b\\[c.pdf](${url})`)
  })
})
