import { describe, expect, it } from 'vitest'
import { appVersion, UNKNOWN_VERSION } from './app-info'
// 版本号只有一个来源：package.json。构建期注入的就是它，测试用同一份真值对齐。
import pkg from '../../package.json'

describe('app info', () => {
  it('reports the version injected from package.json', () => {
    // 注入链断了就会显示「—」，所以这里同时锁住「注入值 == package.json 的版本」。
    expect(appVersion({ VITE_APP_VERSION: pkg.version })).toBe(pkg.version)
  })

  it('never renders an undefined version', () => {
    expect(appVersion({})).toBe(UNKNOWN_VERSION)
    expect(appVersion({ VITE_APP_VERSION: '' })).toBe(UNKNOWN_VERSION)
    expect(appVersion({ VITE_APP_VERSION: '   ' })).toBe(UNKNOWN_VERSION)
    expect(appVersion({ VITE_APP_VERSION: 2103 })).toBe(UNKNOWN_VERSION)
    expect(appVersion({ VITE_APP_VERSION: null })).toBe(UNKNOWN_VERSION)
  })

  it('trims surrounding whitespace', () => {
    expect(appVersion({ VITE_APP_VERSION: ' 2.10.3 ' })).toBe('2.10.3')
  })
})
