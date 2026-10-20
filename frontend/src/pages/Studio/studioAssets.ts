// Copyright © 2026 Jalapeno Labs

import type { StudioAsset } from '../../api/routes/studioRoutes'

// How the stage reads an item's files. Every version of a file is its own asset; versions
// share the path the agent wrote them to. Files of one model share a stem, the path without
// its extension, so `banana.blend` and `banana.glb` are one model whose `.glb` drives the
// viewer. Grouping is done here, not by the API (docs/studio.md).

// One path's versions, newest first.
export type StudioFile = {
  path: string
  versions: StudioAsset[]
}

export type StudioModel = {
  stem: string
  // Each of the model's paths, in the order their newest versions arrived.
  files: StudioFile[]
  // The `.glb` the viewer shows; null when the model has none, such as when its export
  // failed.
  viewerFile: StudioFile | null
}

// The files offered for download under one stem: the newest version of each path.
export type StudioDownloadGroup = {
  stem: string
  assets: StudioAsset[]
}

// The path without its extension. Only the last segment's last dot counts, so a dotted
// directory keeps its name.
export function getAssetStem(path: string) {
  const lastSlash = path.lastIndexOf('/')
  const lastDot = path.lastIndexOf('.')
  if (lastDot <= lastSlash + 1) {
    return path
  }
  return path.slice(0, lastDot)
}

function isViewerPath(path: string) {
  return path.toLowerCase().endsWith('.glb')
}

// `assets` must be newest first, as the slice holds them; every list keeps that order.
// One pass files each asset under its path, and each new path under its kind and stem.
export function groupStudioAssets(assets: StudioAsset[]) {
  const filesByPath = new Map<string, StudioFile>()
  const modelsByStem = new Map<string, StudioModel>()
  const downloadsByStem = new Map<string, StudioDownloadGroup>()
  const images: StudioFile[] = []
  const models: StudioModel[] = []
  const otherFiles: StudioFile[] = []
  const downloads: StudioDownloadGroup[] = []

  for (const asset of assets) {
    const existing = filesByPath.get(asset.artifactPath)
    if (existing) {
      existing.versions.push(asset)
      continue
    }

    const file: StudioFile = { path: asset.artifactPath, versions: [ asset ]}
    filesByPath.set(asset.artifactPath, file)
    const stem = getAssetStem(asset.artifactPath)

    let downloadGroup = downloadsByStem.get(stem)
    if (!downloadGroup) {
      downloadGroup = { stem, assets: []}
      downloadsByStem.set(stem, downloadGroup)
      downloads.push(downloadGroup)
    }
    downloadGroup.assets.push(asset)

    if (asset.kind === 'image') {
      images.push(file)
      continue
    }
    if (asset.kind === 'file') {
      otherFiles.push(file)
      continue
    }

    let model = modelsByStem.get(stem)
    if (!model) {
      model = { stem, files: [], viewerFile: null }
      modelsByStem.set(stem, model)
      models.push(model)
    }
    model.files.push(file)
    if (isViewerPath(file.path)) {
      model.viewerFile = file
    }
  }

  return { images, models, otherFiles, downloads } as const
}

export type StudioAssetGroups = ReturnType<typeof groupStudioAssets>

// What the stage shows: one image path, or one model.
export type StageSubject =
  | { kind: 'image', path: string }
  | { kind: 'model', stem: string }

// What the stage opens on: a model the viewer can show, since a 3D item's model is its
// deliverable; else the image the tile shows; else the newest image; else a model without a
// preview, which still offers its downloads.
export function getDefaultStageSubject(
  groups: StudioAssetGroups,
  thumbnailAssetId: string | null,
): StageSubject | null {
  const viewable = groups.models.find((model) => model.viewerFile)
  if (viewable) {
    return { kind: 'model', stem: viewable.stem }
  }

  const thumbnail = groups.images.find((image) => image.versions.some((version) => version.id === thumbnailAssetId))
  const image = thumbnail ?? groups.images[0]
  if (image) {
    return { kind: 'image', path: image.path }
  }

  const model = groups.models[0]
  if (model) {
    return { kind: 'model', stem: model.stem }
  }
  return null
}

// The versions the filmstrip compares for a subject, newest first: an image path's own, or
// the versions of a model's `.glb`. Empty when the subject is gone or has no preview.
export function getSubjectVersions(groups: StudioAssetGroups, subject: StageSubject): StudioAsset[] {
  if (subject.kind === 'image') {
    return groups.images.find((image) => image.path === subject.path)?.versions ?? []
  }
  return groups.models.find((model) => model.stem === subject.stem)?.viewerFile?.versions ?? []
}
