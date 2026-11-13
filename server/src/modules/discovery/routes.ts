// Server discovery (SPEC §7.1): the desktop app is given one URL and finds
// the API and the sign-in issuer from it.

import type { FastifyPluginAsyncTypebox } from '@fastify/type-provider-typebox'
import { Type } from 'typebox'
import type { Config } from '../../config.ts'

const Discovery = Type.Object(
  {
    apiUrl: Type.String({ description: 'Base URL of the API (without /v1)' }),
    issuer: Type.String({ description: 'OpenID Connect issuer; its /.well-known/openid-configuration lists the endpoints' }),
    clientId: Type.String({ description: 'Public OAuth client the desktop app signs in with' }),
    apiVersions: Type.Array(Type.Integer(), { description: 'API versions this server speaks (/v<n>)' }),
    minClientVersion: Type.String({ description: 'Oldest desktop app version (major.minor.patch) this server works with' }),
    deploymentMode: Type.Union([Type.Literal('cloud'), Type.Literal('self_hosted')]),
  },
  { description: 'Where to sign in and where the API lives' },
)

export const discoveryRoutes: FastifyPluginAsyncTypebox<{ config: Config }> = async (app, { config }) => {
  app.get(
    '/.well-known/zeima-server',
    {
      schema: {
        tags: ['discovery'],
        summary: 'Find the API and the sign-in issuer from the server URL',
        response: { 200: Discovery },
      },
    },
    async () => ({
      apiUrl: config.publicUrl,
      issuer: config.keycloak.issuer,
      clientId: 'zeima-desktop',
      apiVersions: [1],
      minClientVersion: config.minClientVersion,
      deploymentMode: config.deploymentMode,
    }),
  )
}
