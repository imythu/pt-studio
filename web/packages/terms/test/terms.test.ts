import { describe, expect, it } from 'vitest';
import { formatBytes, makeTranslator, registerMessages } from '../src/index';

describe('terms', () => {
  it('resolves terms, parts and ui messages with fallback', () => {
    registerMessages('zh-CN', { 'ui.hello': '你好，{name}' });
    const t = makeTranslator('en', { 'bonus.currency': { name: 'Bonus', short: 'BP', unit: 'pts' } });
    expect(t.t('term:bonus.currency')).toBe('Bonus');
    expect(t.t('term:bonus.currency|short')).toBe('BP');
    expect(t.t('term:bonus.currency|unit')).toBe('pts');
    expect(t.t('term:missing.key')).toBe('missing.key');
    expect(t.t('ui.hello', { name: 'A' })).toBe('你好，A');
    expect(t.t('ui.unknown')).toBe('ui.unknown');
  });

  it('formats bytes', () => {
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(1536)).toBe('1.50 KiB');
    expect(formatBytes(5 * 1024 ** 3)).toBe('5.00 GiB');
  });
});
