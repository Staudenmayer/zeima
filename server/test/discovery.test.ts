import { describe, expect, it } from 'vitest'
import { config, testApp } from './helpers.ts'

describe('GET /.well-known/zeima-server', () => {
  it('tells the desktop app where to sign in, without a token', async () => {
    const app = await testApp()
    const res = await app.inject('/.well-known/zeima-server')
    expect(res.statusCode).toBe(200)
    expect(res.json()).toEqual({
      apiUrl: config.publicUrl,
      issuer: config.keycloak.issuer,
      clientId: 'zeima-desktop',
      apiVersions: [1],
      minClientVersion: '0.0.0',
      deploymentMode: config.deploymentMode,
    })
    await app.close()
  })
})
