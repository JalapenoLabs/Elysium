// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { makeStudioAsset } from '../../testFixtures'
import { getAssetStem, getDefaultStageSubject, getSubjectVersions, groupStudioAssets } from './studioAssets'

describe('getAssetStem', () => {
  it('drops the extension', () => {
    expect(getAssetStem('banana.blend')).toBe('banana')
    expect(getAssetStem('renders/banana-hero.png')).toBe('renders/banana-hero')
  })

  it('drops only the last extension', () => {
    expect(getAssetStem('banana.tar.gz')).toBe('banana.tar')
  })

  it('keeps a dotted directory and a name without an extension whole', () => {
    expect(getAssetStem('v1.2/banana')).toBe('v1.2/banana')
    expect(getAssetStem('notes/.hidden')).toBe('notes/.hidden')
  })
})

// Newest first, as the slice holds them.
const newerGlb = makeStudioAsset({ id: 'glb-2', artifactPath: 'banana.glb', kind: 'model' })
const newerHero = makeStudioAsset({ id: 'hero-2', artifactPath: 'renders/banana-hero.png' })
const blend = makeStudioAsset({ id: 'blend-1', artifactPath: 'banana.blend', kind: 'model' })
const olderGlb = makeStudioAsset({ id: 'glb-1', artifactPath: 'banana.glb', kind: 'model' })
const olderHero = makeStudioAsset({ id: 'hero-1', artifactPath: 'renders/banana-hero.png' })
const front = makeStudioAsset({ id: 'front-1', artifactPath: 'renders/banana-front.png' })
const notes = makeStudioAsset({ id: 'notes-1', artifactPath: 'notes.txt', kind: 'file' })
const assets = [ newerGlb, newerHero, blend, olderGlb, olderHero, front, notes ]

describe('groupStudioAssets', () => {
  const groups = groupStudioAssets(assets)

  it('files every version under its path, newest first', () => {
    expect(groups.images.map((image) => image.path)).toEqual([
      'renders/banana-hero.png',
      'renders/banana-front.png',
    ])
    expect(groups.images[0].versions).toEqual([ newerHero, olderHero ])
  })

  it('makes one model of the files sharing a stem, with its .glb driving the viewer', () => {
    expect(groups.models).toHaveLength(1)
    const [ model ] = groups.models
    expect(model.stem).toBe('banana')
    expect(model.files.map((file) => file.path)).toEqual([ 'banana.glb', 'banana.blend' ])
    expect(model.viewerFile?.versions).toEqual([ newerGlb, olderGlb ])
  })

  it('leaves a model without a .glb with no viewer file', () => {
    const blendOnly = groupStudioAssets([ blend ])
    expect(blendOnly.models[0].viewerFile).toBeNull()
  })

  it('keeps other files apart', () => {
    expect(groups.otherFiles.map((file) => file.path)).toEqual([ 'notes.txt' ])
  })

  it('offers the newest version of each path for download, grouped by stem', () => {
    expect(groups.downloads).toEqual([
      { stem: 'banana', assets: [ newerGlb, blend ]},
      { stem: 'renders/banana-hero', assets: [ newerHero ]},
      { stem: 'renders/banana-front', assets: [ front ]},
      { stem: 'notes', assets: [ notes ]},
    ])
  })
})

describe('getDefaultStageSubject', () => {
  it('opens on a model the viewer can show', () => {
    expect(getDefaultStageSubject(groupStudioAssets(assets), null)).toEqual({ kind: 'model', stem: 'banana' })
  })

  it('opens on the image the tile shows when there is no viewable model', () => {
    const groups = groupStudioAssets([ newerHero, blend, front ])
    expect(getDefaultStageSubject(groups, 'front-1')).toEqual({ kind: 'image', path: 'renders/banana-front.png' })
  })

  it('opens on the newest image when the tile shows none of them', () => {
    const groups = groupStudioAssets([ newerHero, front ])
    expect(getDefaultStageSubject(groups, null)).toEqual({ kind: 'image', path: 'renders/banana-hero.png' })
  })

  it('opens on a model without a preview when it is all there is', () => {
    expect(getDefaultStageSubject(groupStudioAssets([ blend ]), null)).toEqual({ kind: 'model', stem: 'banana' })
  })

  it('has nothing to open on without images or models', () => {
    expect(getDefaultStageSubject(groupStudioAssets([ notes ]), null)).toBeNull()
  })
})

describe('getSubjectVersions', () => {
  const groups = groupStudioAssets(assets)

  it('lists an image path\'s versions', () => {
    const versions = getSubjectVersions(groups, { kind: 'image', path: 'renders/banana-hero.png' })
    expect(versions).toEqual([ newerHero, olderHero ])
  })

  it('lists a model\'s .glb versions', () => {
    expect(getSubjectVersions(groups, { kind: 'model', stem: 'banana' })).toEqual([ newerGlb, olderGlb ])
  })

  it('lists nothing for a subject that is gone', () => {
    expect(getSubjectVersions(groups, { kind: 'image', path: 'missing.png' })).toEqual([])
  })
})
