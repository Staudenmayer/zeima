import { describe, expect, it } from 'vitest'
import { ConfigError, loadConfig } from '../src/config.ts'

describe('loadConfig', () => {
  it('has working defaults for development', () => {
    const c = loadConfig({})
    expect(c.port).toBe(3000)
    expect(c.deploymentMode).toBe('self_hosted')
    expect(c.keycloak.issuer).toBe('http://localhost:8080/realms/zeima')
    expect(c.keycloak.jwksUrl).toBe('http://localhost:8080/realms/zeima/protocol/openid-connect/certs')
    expect(c.allowedClients).toEqual(['zeima-desktop', 'zeima-swagger', 'zeima-web'])
    expect(c.corsOrigins).toEqual([])
  })

  it('converts strings and ignores unrelated variables', () => {
    const c = loadConfig({
      PORT: '8081',
      DOCS_ENABLED: 'false',
      KEYCLOAK_URL: 'https://id.example.com/',
      CORS_ORIGINS: ' https://a.example.com , https://b.example.com ',
      PATH: '/usr/bin',
      HOST: '',
    })
    expect(c.port).toBe(8081)
    expect(c.docsEnabled).toBe(false)
    expect(c.keycloak.issuer).toBe('https://id.example.com/realms/zeima')
    expect(c.corsOrigins).toEqual(['https://a.example.com', 'https://b.example.com'])
    expect(c.host).toBe('0.0.0.0')
  })

  it('fetches keys from the internal Keycloak address but checks the public issuer', () => {
    const c = loadConfig({ KEYCLOAK_URL: 'http://localhost:8080', KEYCLOAK_INTERNAL_URL: 'http://keycloak:8080/' })
    expect(c.keycloak.issuer).toBe('http://localhost:8080/realms/zeima')
    expect(c.keycloak.jwksUrl).toBe('http://keycloak:8080/realms/zeima/protocol/openid-connect/certs')
    expect(c.keycloak.authorizationUrl).toBe('http://localhost:8080/realms/zeima/protocol/openid-connect/auth')
  })

  it('lists every problem at once', () => {
    let error: unknown
    try {
      loadConfig({ PORT: 'eighty', DEPLOYMENT_MODE: 'moon' })
    } catch (e) {
      error = e
    }
    expect(error).toBeInstanceOf(ConfigError)
    const text = (error as ConfigError).problems.join('\n')
    expect(text).toContain('PORT')
    expect(text).toContain('DEPLOYMENT_MODE')
  })
})

describe('MIN_CLIENT_VERSION', () => {
  it('defaults to every version and must look like major.minor.patch', () => {
    expect(loadConfig({}).minClientVersion).toBe('0.0.0')
    expect(loadConfig({ MIN_CLIENT_VERSION: '1.1.0' }).minClientVersion).toBe('1.1.0')
    expect(() => loadConfig({ MIN_CLIENT_VERSION: 'new' })).toThrow(ConfigError)
  })
})
