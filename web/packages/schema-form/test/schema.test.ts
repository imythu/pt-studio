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
