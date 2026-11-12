// OpenAPI 3.1 document from the route schemas, and the Swagger UI at /docs
// with an "Authorize" button against Keycloak (PKCE).

import swagger from '@fastify/swagger'
import swaggerUi from '@fastify/swagger-ui'
import type { FastifyInstance } from 'fastify'
import fp from 'fastify-plugin'
import type { Config } from '../config.ts'

export const API_VERSION = '0.1.0'

export const docsPlugin = fp(
  async (app: FastifyInstance, { config }: { config: Config }) => {
    await app.register(swagger, {
      openapi: {
        openapi: '3.1.0',
        info: {
          title: 'Zeima API',
          version: API_VERSION,
          description:
            'Sync, organizations and licensing for the Zeima desktop app. Errors are `application/problem+json` (RFC 9457) with a stable `code`.',
        },
        servers: [{ url: config.publicUrl }],
        tags: [
          { name: 'me', description: 'The signed-in user' },
          { name: 'health', description: 'Liveness and readiness' },
        ],
        components: {
          securitySchemes: {
            keycloak: {
              type: 'oauth2',
              description: 'Keycloak access token (`Authorization: Bearer …`)',
              flows: {
                authorizationCode: {
                  authorizationUrl: config.keycloak.authorizationUrl,
                  tokenUrl: config.keycloak.tokenUrl,
                  scopes: { openid: 'OpenID Connect' },
                },
              },
            },
          },
        },
      },
      // Shared schemas keep their `$id` as the component name (e.g. Problem).
      refResolver: {
        buildLocalReference: (json, _baseUri, _fragment, i) => (typeof json.$id === 'string' ? json.$id : `def-${i}`),
      },
    })

    if (config.docsEnabled) {
      await app.register(swaggerUi, {
        routePrefix: '/docs',
        staticCSP: true,
        uiConfig: { persistAuthorization: false, displayRequestDuration: true },
        initOAuth: {
          clientId: config.docsClientId,
          usePkceWithAuthorizationCodeGrant: true,
          scopes: ['openid'],
        },
      })
    }
  },
  { name: 'docs' },
)
