// Copyright © 2026 Jalapeno Labs

import type {
  Integration,
  IntegrationKind,
  IntegrationMode,
  IntegrationStorageBinding,
  IntegrationVisibility,
  ListIntegrationsResponse,
  OwnerScope,
} from '../../proto/generated/integration_pb'

// Lib
import { create, fromBinary } from '@bufbuild/protobuf'

// Misc
import { apiClient, getProto, sendProto } from '../index'
import {
  CreateIntegrationRequestSchema,
  IntegrationSchema,
  ListIntegrationsResponseSchema,
  SetIntegrationBindingsRequestSchema,
  UpdateIntegrationRequestSchema,
} from '../../proto/generated/integration_pb'

// Integrations: the service control plane (docs/integrations.md). Managed +
// org-linked mutations are gated on `manage_integrations`; personal linking is
// policy-gated per kind (#36). The provider credential is write-only.

// Omit projectId to list every integration reachable in the active org.
export function listIntegrations(projectId?: string): Promise<ListIntegrationsResponse> {
  const query = projectId ? `?project_id=${projectId}` : ''
  return getProto(`api/v1/integrations${query}`, ListIntegrationsResponseSchema)
}

type CreateIntegrationFields = {
  projectId: string
  kind: IntegrationKind
  mode: IntegrationMode
  ownerScope: OwnerScope
  name: string
  visibility: IntegrationVisibility
  configJson: string
  credential: string
}

export function createIntegration(fields: CreateIntegrationFields): Promise<Integration> {
  return sendProto('api/v1/integrations', {
    method: 'post',
    requestSchema: CreateIntegrationRequestSchema,
    request: create(CreateIntegrationRequestSchema, {
      projectId: { value: fields.projectId },
      kind: fields.kind,
      mode: fields.mode,
      ownerScope: fields.ownerScope,
      name: fields.name,
      visibility: fields.visibility,
      configJson: fields.configJson,
      credential: fields.credential,
    }),
    responseSchema: IntegrationSchema,
  })
}

type UpdateIntegrationFields = {
  name: string
  visibility: IntegrationVisibility
  configJson: string
  // Empty keeps the existing sealed credential.
  credential: string
}

export function updateIntegration(
  integrationId: string,
  fields: UpdateIntegrationFields,
): Promise<Integration> {
  return sendProto(`api/v1/integrations/${integrationId}`, {
    method: 'patch',
    requestSchema: UpdateIntegrationRequestSchema,
    request: create(UpdateIntegrationRequestSchema, {
      name: fields.name,
      visibility: fields.visibility,
      configJson: fields.configJson,
      credential: fields.credential,
    }),
    responseSchema: IntegrationSchema,
  })
}

export function deleteIntegration(integrationId: string) {
  return apiClient.delete(`api/v1/integrations/${integrationId}`)
}

// Managed integrations only; both return the integration with its live status.
export async function startIntegration(integrationId: string): Promise<Integration> {
  const buffer = await apiClient.post(`api/v1/integrations/${integrationId}/start`).arrayBuffer()
  return fromBinary(IntegrationSchema, new Uint8Array(buffer))
}

export async function stopIntegration(integrationId: string): Promise<Integration> {
  const buffer = await apiClient.post(`api/v1/integrations/${integrationId}/stop`).arrayBuffer()
  return fromBinary(IntegrationSchema, new Uint8Array(buffer))
}

// Full replace of the integration's storage-role bindings.
export function setIntegrationBindings(
  integrationId: string,
  bindings: IntegrationStorageBinding[],
): Promise<Integration> {
  return sendProto(`api/v1/integrations/${integrationId}/bindings`, {
    method: 'put',
    requestSchema: SetIntegrationBindingsRequestSchema,
    request: create(SetIntegrationBindingsRequestSchema, { bindings }),
    responseSchema: IntegrationSchema,
  })
}
