// Copyright © 2026 Jalapeno Labs

import type {
  ListProjectMembersResponse,
  ListProjectsResponse,
  Project,
  ProjectRole,
} from '../../proto/generated/project_pb'

// Lib
import { create, toBinary } from '@bufbuild/protobuf'

// Misc
import { apiClient, getProto, sendProto } from '../index'
import {
  AddProjectDomainRequestSchema,
  CreateProjectRequestSchema,
  ListProjectMembersResponseSchema,
  ListProjectsResponseSchema,
  ProjectSchema,
  SetProjectMemberRequestSchema,
  UpdateProjectRequestSchema,
} from '../../proto/generated/project_pb'

// Projects: the sub-tenant workspaces of the active org (docs/projects.md).
// List/create operate on the session's active org; protobuf bodies both ways.

export function listProjects(): Promise<ListProjectsResponse> {
  return getProto('api/v1/projects', ListProjectsResponseSchema)
}

export function createProject(name: string, slug?: string): Promise<Project> {
  return sendProto('api/v1/projects', {
    method: 'post',
    requestSchema: CreateProjectRequestSchema,
    request: create(CreateProjectRequestSchema, { name, slug: slug ?? '' }),
    responseSchema: ProjectSchema,
  })
}

// Empty fields leave the current value unchanged.
export function updateProject(projectId: string, fields: { name?: string, slug?: string }): Promise<Project> {
  return sendProto(`api/v1/projects/${projectId}`, {
    method: 'patch',
    requestSchema: UpdateProjectRequestSchema,
    request: create(UpdateProjectRequestSchema, {
      name: fields.name ?? '',
      slug: fields.slug ?? '',
    }),
    responseSchema: ProjectSchema,
  })
}

// 409 when it is the org's last project (every org keeps at least one).
export function deleteProject(projectId: string) {
  return apiClient.delete(`api/v1/projects/${projectId}`)
}

export function listProjectMembers(projectId: string): Promise<ListProjectMembersResponse> {
  return getProto(`api/v1/projects/${projectId}/members`, ListProjectMembersResponseSchema)
}

// Adds the user or changes their role; 204 either way.
export function setProjectMember(projectId: string, userId: string, role: ProjectRole) {
  return apiClient.put(`api/v1/projects/${projectId}/members/${userId}`, {
    body: toBinary(
      SetProjectMemberRequestSchema,
      create(SetProjectMemberRequestSchema, { role }),
    ),
    headers: { 'content-type': 'application/x-protobuf' },
  })
}

export function removeProjectMember(projectId: string, userId: string) {
  return apiClient.delete(`api/v1/projects/${projectId}/members/${userId}`)
}

// 409 when the domain is already claimed by another project.
export function addProjectDomain(projectId: string, domain: string): Promise<Project> {
  return sendProto(`api/v1/projects/${projectId}/domains`, {
    method: 'post',
    requestSchema: AddProjectDomainRequestSchema,
    request: create(AddProjectDomainRequestSchema, { domain }),
    responseSchema: ProjectSchema,
  })
}

export function removeProjectDomain(projectId: string, domain: string) {
  return apiClient.delete(`api/v1/projects/${projectId}/domains/${encodeURIComponent(domain)}`)
}
