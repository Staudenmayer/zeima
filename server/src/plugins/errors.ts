// Errors as `application/problem+json` (RFC 9457) with a stable `code`
// clients can switch on (SPEC §7.0).

import type { FastifyError, FastifyInstance } from 'fastify'
import fp from 'fastify-plugin'
import { type Static, Type } from 'typebox'

export const ProblemSchema = Type.Object(
  {
    type: Type.String({ description: 'Always `about:blank`; use `code`.' }),
    title: Type.String(),
    status: Type.Integer(),
    code: Type.String({ description: 'Stable machine-readable error code, e.g. `unauthorized`.' }),
    detail: Type.Optional(Type.String()),
    errors: Type.Optional(
      Type.Array(Type.Object({ path: Type.String(), message: Type.String() }), {
        description: 'Validation problems (`validation_failed`).',
      }),
    ),
  },
  { $id: 'Problem', description: 'RFC 9457 problem details' },
)
export type Problem = Static<typeof ProblemSchema>

const PROBLEM_RESPONSE = {
  description: 'Problem details (RFC 9457)',
  content: { 'application/problem+json': { schema: Type.Ref('Problem') } },
}

/** Response schemas for the given error statuses, for route definitions. */
export function problems<S extends number>(...statuses: S[]): Record<S, typeof PROBLEM_RESPONSE> {
  return Object.fromEntries(statuses.map((s) => [s, PROBLEM_RESPONSE])) as Record<S, typeof PROBLEM_RESPONSE>
}

const TITLES: Record<number, string> = {
  400: 'Bad Request',
  401: 'Unauthorized',
  403: 'Forbidden',
  404: 'Not Found',
  409: 'Conflict',
  413: 'Content Too Large',
  415: 'Unsupported Media Type',
  422: 'Unprocessable Content',
  429: 'Too Many Requests',
  500: 'Internal Server Error',
  503: 'Service Unavailable',
}

/** An error the client caused or should know about; becomes a problem response. */
export class HttpProblem extends Error {
  readonly status: number
  readonly code: string
  readonly headers: Record<string, string>

  constructor(status: number, code: string, detail?: string, headers: Record<string, string> = {}) {
    super(detail ?? TITLES[status] ?? 'Error')
    this.name = 'HttpProblem'
    this.status = status
    this.code = code
    this.headers = headers
  }
}

export function problem(status: number, code: string, detail?: string): Problem {
  return {
    type: 'about:blank',
    title: TITLES[status] ?? 'Error',
    status,
    code,
    ...(detail ? { detail } : {}),
  }
}

/** Fastify's own client errors (FST_ERR_*) by status. */
const CLIENT_CODES: Record<number, string> = {
  400: 'bad_request',
  404: 'not_found',
  413: 'payload_too_large',
  415: 'unsupported_media_type',
  429: 'rate_limited',
}

export const errorsPlugin = fp(
  async (app: FastifyInstance) => {
    app.addSchema(ProblemSchema)

    app.setErrorHandler((error: FastifyError | HttpProblem, request, reply) => {
      let body: Problem
      if (error instanceof HttpProblem) {
        body = problem(error.status, error.code, error.message)
        reply.headers(error.headers)
      } else if (error.validation) {
        body = {
          ...problem(400, 'validation_failed', error.message),
          errors: error.validation.map((v) => ({
            path: `${error.validationContext ?? ''}${v.instancePath}`,
            message: v.message ?? 'invalid',
          })),
        }
      } else if (error.statusCode && error.statusCode >= 400 && error.statusCode < 500) {
        body = problem(error.statusCode, CLIENT_CODES[error.statusCode] ?? 'bad_request', error.message)
      } else {
        // Details of server errors stay in the log.
        request.log.error({ err: error }, 'request failed')
        body = problem(500, 'internal')
      }
      return reply.code(body.status).type('application/problem+json').send(body)
    })

    app.setNotFoundHandler((request, reply) =>
      reply
        .code(404)
        .type('application/problem+json')
        .send(problem(404, 'not_found', `${request.method} ${request.url.split('?')[0]} does not exist`)),
    )
  },
  { name: 'errors' },
)
