// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { isOwnMeetingChange, trackMeetingChange } from './meetingChangeEchoes'

describe('meetingChangeEchoes', () => {
  it('knows a change the page made once it answers, and only once', async () => {
    await trackMeetingChange('meeting', Promise.resolve({ entry: { id: 'mine' }}))

    expect(await isOwnMeetingChange('meeting', 'mine')).toBe(true)
    // A later event naming it again is news, such as the same change seen from another tab.
    expect(await isOwnMeetingChange('meeting', 'mine')).toBe(false)
    expect(await isOwnMeetingChange('meeting', 'someone-elses')).toBe(false)
  })

  it('waits for a change still in flight when its announcement arrives first', async () => {
    let answer: (value: { entry: { id: string } }) => void = () => {}
    const request = new Promise<{ entry: { id: string } }>((resolve) => {
      answer = resolve
    })
    void trackMeetingChange('meeting', request)

    const asked = isOwnMeetingChange('meeting', 'early')
    answer({ entry: { id: 'early' }})

    expect(await asked).toBe(true)
  })

  it('reads an announcement as news when the change it waited on failed', async () => {
    const failing = trackMeetingChange('meeting', Promise.reject(new Error('refused')))

    expect(await isOwnMeetingChange('meeting', 'whatever')).toBe(false)
    await expect(failing).rejects.toThrow('refused')
  })
})
