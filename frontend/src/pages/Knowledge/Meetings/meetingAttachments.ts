// Copyright © 2026 Jalapeno Labs

import type { MeetingAttachment, MeetingMark, MeetingWord } from '../../../api/routes/meetingRoutes'
import type { WordRange } from './meetingTranscript'

// A meeting's files and frames as its page reads them (docs/meeting-review.md, Attachments and
// frames). A pin is a mark of kind `pin` over a range of the words, naming the file or frame it
// pins in its data; an excerpt carries every file and frame pinned over its words. Everything here
// is pure, so the page's components only draw what it answers.

// The file or frame `mark` pins, when it is a pin.
export function attachmentIdOfPin(mark: MeetingMark): string | null {
  if (mark.kind !== 'pin') {
    return null
  }
  const attachmentId = mark.data.attachmentId
  return typeof attachmentId === 'string'
    ? attachmentId
    : null
}

// Where each pin lies on the transcript as it reads, by word index, for the file or frame it pins.
// A pin over a word that no longer stands is left out until the transcript is read again.
export function placePins(words: MeetingWord[], marks: MeetingMark[]) {
  const indexById = new Map<string, number>()
  for (const [ index, word ] of words.entries()) {
    indexById.set(word.id, index)
  }
  const placed: { attachmentId: string, markId: string, range: WordRange }[] = []
  for (const mark of marks) {
    const attachmentId = attachmentIdOfPin(mark)
    const start = indexById.get(mark.startWordId)
    const end = indexById.get(mark.endWordId)
    if (attachmentId === null || start === undefined || end === undefined) {
      continue
    }
    placed.push({ attachmentId, markId: mark.id, range: { start, end }})
  }
  return placed
}

export type PlacedPin = ReturnType<typeof placePins>[number]

// The files and frames pinned over `range`, each once, in the order they arrived: what an excerpt
// over those words carries to its projects, as the API carries it. A pin lies over a range when
// their words overlap.
export function pinnedOver(
  pins: PlacedPin[],
  attachments: MeetingAttachment[],
  range: WordRange,
): MeetingAttachment[] {
  const pinnedIds = new Set<string>()
  for (const pin of pins) {
    if (pin.range.start <= range.end && range.start <= pin.range.end) {
      pinnedIds.add(pin.attachmentId)
    }
  }
  return attachments.filter((attachment) => pinnedIds.has(attachment.id))
}

// The words the first pin of `attachmentId` lies over, which choosing it selects; null for a file
// or frame pinned nowhere.
export function firstPinOf(pins: PlacedPin[], attachmentId: string): PlacedPin | null {
  let first: PlacedPin | null = null
  for (const pin of pins) {
    if (pin.attachmentId === attachmentId && (!first || pin.range.start < first.range.start)) {
      first = pin
    }
  }
  return first
}

// A meeting's files and frames as its lane lists them: the frames in the order they were shown,
// then the files in the order they arrived.
export function orderAttachments(attachments: MeetingAttachment[]) {
  const frames: MeetingAttachment[] = []
  const files: MeetingAttachment[] = []
  for (const attachment of attachments) {
    if (attachment.kind === 'frame') {
      frames.push(attachment)
    }
    else {
      files.push(attachment)
    }
  }
  frames.sort((first, second) => (first.meetingMs ?? 0) - (second.meetingMs ?? 0))
  return { frames, files } as const
}

// Why a file is not sent: empty, larger than the meeting takes, or with nowhere to keep it.
export type AttachRefusal = 'empty' | 'tooLarge' | 'nowhere'

// Whether a meeting takes `file` before a byte of it is sent, as the API decides again.
export function refusalOf(
  file: Pick<File, 'size'>,
  policy: { maxBytes: number, destinationId: string | null },
): AttachRefusal | null {
  if (!policy.destinationId) {
    return 'nowhere'
  }
  if (!file.size) {
    return 'empty'
  }
  if (file.size > policy.maxBytes) {
    return 'tooLarge'
  }
  return null
}
