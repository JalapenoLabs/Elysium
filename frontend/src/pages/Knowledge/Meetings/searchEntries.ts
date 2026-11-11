// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../../search/searchTypes'

// User interface
import { LuUpload, LuVideo } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../../urls'

// The knowledge base's Meetings page and uploading a meeting from it, for its members. Each meeting
// is found by the knowledge search as its topics. See src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'knowledgeMeetings',
    group: 'pages',
    title: 'meetings:title',
    parent: 'knowledge:title',
    tags: 'search:tags.meetings',
    icon: LuVideo,
    audience: 'members',
    href: UrlTree.knowledgeMeetings,
  },
  {
    id: 'uploadMeeting',
    group: 'actions',
    title: 'meetings:upload.action',
    parent: 'meetings:title',
    tags: 'search:tags.meetingUpload',
    icon: LuUpload,
    audience: 'members',
    href: UrlTree.meetingUploadNew,
  },
]
