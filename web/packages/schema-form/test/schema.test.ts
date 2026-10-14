import { describe, expect, it } from 'vitest';
import { serverErrors, validate, withDefaults, type Schema } from '../src/index';

const schema: Schema = {
  type: 'object',
  required: ['window_days'],
  additionalProperties: false,
  properties: {
    window_days: { type: 'integer', minimum: 1, maximum: 60, default: 14 },
    mode: { type: 'string', enum: ['ignore', 'new_record'] },
    rules: { type: 'array', items: { type: 'object', required: ['id'], properties: { id: { type: 'string', pattern: '^[a-z_]+$' } } } },
  },
};

describe('schema-form', () => {
  it('validates the supported subset with JSON Pointer paths', () => {
    expect(validate(schema, { window_days: 14 })).toEqual({});
    expect(validate(schema, {})).toEqual({ '/window_days': 'required' });
    expect(validate(schema, { window_days: 0 })['/window_days']).toMatch(/≥ 1/);
    expect(validate(schema, { window_days: 1.5 })['/window_days']).toMatch(/expected integer/);
    expect(validate(schema, { window_days: 3, mode: 'x' })['/mode']).toBeDefined();
    expect(validate(schema, { window_days: 3, extra: 1 })['/extra']).toBe('unknown field');
    expect(validate(schema, { window_days: 3, rules: [{ id: 'Bad' }, {}] })).toEqual({ '/rules/0/id': 'invalid format', '/rules/1/id': 'required' });
  });

  it('fills defaults and maps server errors', () => {
    expect(withDefaults(schema, {})).toEqual({ window_days: 14 });
    expect(serverErrors({ errors: [{ path: '/mode', message: 'nope' }] })).toEqual({ '/mode': 'nope' });
  });
});

describe('oneOf (type-generic kinds)', () => {
  const generic: Schema = {
    type: 'object',
    required: ['kind'],
    properties: { kind: { type: 'string', enum: ['game', 'ebook'] } },
    oneOf: [
      { type: 'object', required: ['kind', 'platform'], properties: { kind: { const: 'game' }, platform: { type: 'string', enum: ['windows', 'linux'] } } },
      { type: 'object', required: ['kind', 'isbn'], properties: { kind: { const: 'ebook' }, isbn: { type: 'string', maxLength: 17 } } },
    ],
  };
  it('validates the branch selected by the discriminator', async () => {
    const { branchOf, discriminator } = await import('../src/index');
    expect(discriminator(generic)).toBe('kind');
    expect(branchOf(generic, { kind: 'ebook' })?.required).toEqual(['kind', 'isbn']);
    expect(validate(generic, { kind: 'game', platform: 'windows' })).toEqual({});
    expect(validate(generic, { kind: 'game' })).toEqual({ '/platform': 'required' });
    expect(validate(generic, { kind: 'ebook', isbn: 'x'.repeat(20) })['/isbn']).toMatch(/at most/);
    expect(validate(generic, {})).toEqual({ '/kind': 'required' });
  });
});

describe('decodeText (rip logs)', () => {
  it('decodes UTF-16 LE with BOM and plain UTF-8', async () => {
    const { decodeText } = await import('../src/index');
    const s = 'Exact Audio Copy V1.6';
    const le = new Uint8Array(2 + s.length * 2);
    le.set([0xff, 0xfe]);
    for (let i = 0; i < s.length; i++) le[2 + i * 2] = s.charCodeAt(i);
    expect(decodeText(le.buffer)).toBe(s);
    expect(decodeText(new TextEncoder().encode('日志').buffer as ArrayBuffer)).toBe('日志');
  });
});

describe('settings helpers', () => {
  it('compares deeply ignoring key order and undefined', async () => {
    const { same, at } = await import('../src/index');
    expect(same({ a: 1, b: [1, { c: 2 }] }, { b: [1, { c: 2 }], a: 1 })).toBe(true);
    expect(same({ a: 1, x: undefined }, { a: 1 })).toBe(true);
    expect(same({ a: 1 }, { a: 2 })).toBe(false);
    expect(at({ rules: [{ id: 'x' }], 'a/b': 1 }, '/rules/0/id')).toBe('x');
    expect(at({ 'a/b': 1 }, '/a~1b')).toBe(1);
    expect(at({ a: 1 }, '/a/b')).toBeUndefined();
  });

  it('leaves nullable objects unset and validates them only when enabled', () => {
    const s: Schema = {
      type: 'object',
      properties: {
        grant: { type: ['object', 'null'], required: ['amount'], properties: { amount: { type: 'integer', default: 1 } } },
        quota: { type: 'object', properties: { amount: { type: 'integer', default: 0 } } },
      },
    };
    expect(withDefaults(s, {})).toEqual({ quota: { amount: 0 } });
    expect(validate(s, { grant: null })).toEqual({});
    expect(validate(s, { grant: {} })).toEqual({ '/grant/amount': 'required' });
  });
});
