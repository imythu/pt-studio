// @pt-studio/schema-form：JSON Schema 驱动的表单（web LLD §4）。
//
// 校验器为无 eval 的子集实现（CSP 不允许 `unsafe-eval`，Ajv 需要代码生成）：type、required、enum、minimum / maximum、
// minLength / maxLength、pattern、items、properties、additionalProperties: false。

import { useState, type ReactNode } from 'react';

export interface Schema {
  type?: string | string[];
  title?: string;
  description?: string;
  properties?: Record<string, Schema>;
  required?: string[];
  items?: Schema;
  enum?: unknown[];
  default?: unknown;
  minimum?: number;
  maximum?: number;
  minLength?: number;
  maxLength?: number;
  pattern?: string;
  format?: string;
  additionalProperties?: boolean | Schema;
  'x-pt-label-term'?: string;
  'x-pt-widget'?: string;
  'x-pt-secret'?: boolean;
  'x-pt-enum-titles'?: string[];
  /** 属性显示顺序（JSON 对象经服务端后不保证键序） */
  'x-pt-order'?: string[];
  'x-pt-filter'?: string;
  /** 只由服务端写入（如 log 分数），表单不显示 */
  readOnly?: boolean;
  const?: unknown;
  oneOf?: Schema[];
}

export type Errors = Record<string, string>;

function typeOf(v: unknown): string {
  if (v === null) return 'null';
  if (Array.isArray(v)) return 'array';
  if (typeof v === 'number') return Number.isInteger(v) ? 'integer' : 'number';
  return typeof v;
}

function typeOk(want: string, v: unknown): boolean {
  const got = typeOf(v);
  return want === got || (want === 'number' && got === 'integer');
}

/** 校验；键为 JSON Pointer（如 `/class_rules/max_outstanding`） */
export function validate(schema: Schema, value: unknown, path = ''): Errors {
  const errs: Errors = {};
  const put = (p: string, m: string) => {
    if (!(p in errs)) errs[p] = m;
  };
  if (value === undefined) return errs;
  const types = schema.type === undefined ? [] : Array.isArray(schema.type) ? schema.type : [schema.type];
  if (types.length > 0 && !types.some((t) => typeOk(t, value))) {
    put(path, `expected ${types.join(' | ')}`);
    return errs;
  }
  if (schema.enum && !schema.enum.some((e) => JSON.stringify(e) === JSON.stringify(value))) put(path, 'not an allowed value');
  if (typeof value === 'number') {
    if (schema.minimum !== undefined && value < schema.minimum) put(path, `must be ≥ ${schema.minimum}`);
    if (schema.maximum !== undefined && value > schema.maximum) put(path, `must be ≤ ${schema.maximum}`);
  }
  if (typeof value === 'string') {
    const n = [...value].length;
    if (schema.minLength !== undefined && n < schema.minLength) put(path, `at least ${schema.minLength} characters`);
    if (schema.maxLength !== undefined && n > schema.maxLength) put(path, `at most ${schema.maxLength} characters`);
    // 外部 ID 允许粘贴链接，由服务端提取
    if (schema.pattern && schema['x-pt-widget'] !== 'external-id' && !new RegExp(schema.pattern, 'u').test(value)) put(path, 'invalid format');
  }
  if (Array.isArray(value) && schema.items) {
    value.forEach((v, i) => Object.assign(errs, validate(schema.items as Schema, v, `${path}/${i}`)));
  }
  if (schema.oneOf) {
    const b = branchOf(schema, value);
    if (b) Object.assign(errs, validate(b, value, path));
    else if (discriminator(schema)) put(`${path}/${discriminator(schema)}`, 'required');
    return errs;
  }
  if (schema.const !== undefined && JSON.stringify(schema.const) !== JSON.stringify(value)) put(path, 'not an allowed value');
  if (typeOf(value) === 'object') {
    const obj = value as Record<string, unknown>;
    for (const r of schema.required ?? []) if (obj[r] === undefined) put(`${path}/${r}`, 'required');
    for (const [k, v] of Object.entries(obj)) {
      const sub = schema.properties?.[k];
      if (sub) Object.assign(errs, validate(sub, v, `${path}/${k}`));
      else if (schema.additionalProperties === false) put(`${path}/${k}`, 'unknown field');
      else if (typeof schema.additionalProperties === 'object') Object.assign(errs, validate(schema.additionalProperties, v, `${path}/${k}`));
    }
  }
  return errs;
}

/** 服务端 `details.errors[]`（`{path, message}`）→ 字段错误 */
export function serverErrors(details: unknown): Errors {
  const list = (details as { errors?: { path?: string; message?: string; code?: string }[] } | null)?.errors ?? [];
  const out: Errors = {};
  for (const e of list) if (e.path) out[e.path] = e.message ?? e.code ?? 'invalid';
  return out;
}

/** 按 schema 默认值补全 */
export function withDefaults(schema: Schema, value: unknown): unknown {
  if (value === undefined && schema.default !== undefined) return schema.default;
  if (schema.properties && (value === undefined || typeOf(value) === 'object')) {
    const obj = { ...((value as Record<string, unknown>) ?? {}) };
    for (const [k, s] of Object.entries(schema.properties)) {
      const v = withDefaults(s, obj[k]);
      if (v !== undefined) obj[k] = v;
    }
    return obj;
  }
  return value;
}

interface FieldProps {
  schema: Schema;
  value: unknown;
  onChange(v: unknown): void;
  path: string;
  errors: Errors;
  label: string;
  t(k: string): string;
}

function primary(schema: Schema): string {
  return (Array.isArray(schema.type) ? schema.type.find((t) => t !== 'null') : schema.type) ?? 'string';
}

/** 显示名：术语优先；术语缺失（t 原样返回键）时用 schema 的 title */
function labelOf(name: string, s: Schema, t: (k: string) => string): string {
  const term = s['x-pt-label-term'];
  if (term) {
    const v = t(`term:${term}`);
    if (v !== term) return v;
  }
  return s.title ?? name;
}

/** `oneOf` 判别：按 `properties.<key>.const` 选分支（type-generic 的 kind） */
export function discriminator(schema: Schema): string | null {
  const first = schema.oneOf?.[0]?.properties;
  if (!first) return null;
  return Object.entries(first).find(([, s]) => s.const !== undefined)?.[0] ?? null;
}

export function branchOf(schema: Schema, value: unknown): Schema | undefined {
  const d = discriminator(schema);
  if (!d) return undefined;
  const cur = (value as Record<string, unknown> | undefined)?.[d];
  return schema.oneOf?.find((b) => b.properties?.[d]?.const === cur);
}

function enumLabel(schema: Schema, o: unknown, t: (k: string) => string): string {
  const term = schema['x-pt-label-term'];
  if (term) {
    const k = `${term}.${String(o)}`;
    const v = t(`term:${k}`);
    if (v !== k) return v;
  }
  return String(o);
}

/** 按 `x-pt-order` 排列属性，其余按原顺序附后 */
export function ordered(schema: Schema): [string, Schema][] {
  const entries = Object.entries(schema.properties ?? {});
  const order = schema['x-pt-order'];
  if (!order) return entries;
  const rank = (k: string) => {
    const i = order.indexOf(k);
    return i < 0 ? order.length : i;
  };
  return [...entries].sort((a, b) => rank(a[0]) - rank(b[0]));
}

function FieldWrap({ label, error, hint, children }: { label: string; error?: string; hint?: string; children: ReactNode }) {
  return (
    <div className={`pt-field${error ? ' pt-field-invalid' : ''}`}>
      <span className="pt-field-label">{label}</span>
      {children}
      {hint && !error && <span className="pt-field-hint">{hint}</span>}
      {error && <span className="pt-field-error">{error}</span>}
    </div>
  );
}

function SchemaField({ schema, value, onChange, path, errors, label, t }: FieldProps) {
  const err = errors[path];
  const ty = primary(schema);
  const id = `sf${path.replace(/\W/g, '_')}`;
  if (schema.oneOf && discriminator(schema)) {
    const d = discriminator(schema) as string;
    const branch = branchOf(schema, value);
    const sel = schema.properties?.[d] ?? { type: 'string', enum: schema.oneOf.map((b) => b.properties?.[d]?.const) };
    const obj = (value as Record<string, unknown>) ?? {};
    return (
      <div className="pt-oneof">
        <SchemaField
          schema={sel}
          path={`${path}/${d}`}
          label={labelOf(d, sel, t)}
          value={obj[d]}
          onChange={(v) => onChange({ [d]: v })}
          errors={errors}
          t={t}
        />
        {branch && (
          <SchemaField
            schema={{ ...branch, properties: Object.fromEntries(Object.entries(branch.properties ?? {}).filter(([k]) => k !== d)) }}
            path={path}
            label=""
            value={obj}
            onChange={(v) => onChange({ ...(v as Record<string, unknown>), [d]: obj[d] })}
            errors={errors}
            t={t}
          />
        )}
      </div>
    );
  }
  if (ty === 'object' && schema.properties) {
    return (
      <fieldset className="pt-fieldset">
        {path && <legend>{label}</legend>}
        {ordered(schema).filter(([, s]) => !s.readOnly).map(([k, s]) => (
          <SchemaField
            key={k}
            schema={s}
            path={`${path}/${k}`}
            label={labelOf(k, s, t)}
            value={(value as Record<string, unknown> | undefined)?.[k]}
            onChange={(v) => onChange({ ...((value as Record<string, unknown>) ?? {}), [k]: v })}
            errors={errors}
            t={t}
          />
        ))}
      </fieldset>
    );
  }
  if (ty === 'boolean') {
    return (
      <FieldWrap label={label} error={err} hint={schema.description}>
        <input id={id} type="checkbox" checked={value === true} onChange={(e) => onChange(e.target.checked)} />
      </FieldWrap>
    );
  }
  if (schema.enum) {
    return (
      <FieldWrap label={label} error={err} hint={schema.description}>
        <select id={id} className="pt-input" value={value === undefined ? '' : JSON.stringify(value)} onChange={(e) => onChange(e.target.value === '' ? undefined : JSON.parse(e.target.value))}>
          <option value="">—</option>
          {schema.enum.map((o, i) => (
            <option key={JSON.stringify(o)} value={JSON.stringify(o)}>
              {schema['x-pt-enum-titles']?.[i] ?? enumLabel(schema, o, t)}
            </option>
          ))}
        </select>
      </FieldWrap>
    );
  }
  if (ty === 'array' && schema.items?.enum) {
    const cur = Array.isArray(value) ? value : [];
    return (
      <FieldWrap label={label} error={err} hint={schema.description}>
        <div className="pt-checks">
          {schema.items.enum.map((o) => (
            <label key={String(o)} className="pt-check">
              <input type="checkbox" checked={cur.includes(o)} onChange={(e) => onChange(e.target.checked ? [...cur, o] : cur.filter((x) => x !== o))} /> {String(o)}
            </label>
          ))}
        </div>
      </FieldWrap>
    );
  }
  if (ty === 'array' && schema['x-pt-widget'] === 'log-files') {
    return <LogFiles label={label} error={err} hint={schema.description} value={Array.isArray(value) ? (value as string[]) : []} onChange={onChange} />;
  }
  if (ty === 'array' && (!schema.items || primary(schema.items) === 'string')) {
    const cur = Array.isArray(value) ? (value as string[]) : [];
    return (
      <FieldWrap label={label} error={err} hint={schema.description ?? 'comma separated'}>
        <input id={id} className="pt-input" value={cur.join(', ')} onChange={(e) => onChange(e.target.value.split(',').map((s) => s.trim()).filter(Boolean))} />
      </FieldWrap>
    );
  }
  if (ty === 'integer' || ty === 'number') {
    return (
      <FieldWrap label={label} error={err} hint={schema.description}>
        <input
          id={id}
          className="pt-input"
          type="number"
          step={ty === 'integer' ? 1 : 'any'}
          min={schema.minimum}
          max={schema.maximum}
          value={typeof value === 'number' ? value : ''}
          onChange={(e) => onChange(e.target.value === '' ? undefined : ty === 'integer' ? parseInt(e.target.value, 10) : Number(e.target.value))}
        />
      </FieldWrap>
    );
  }
  if (ty === 'string') {
    const multiline = schema['x-pt-widget'] === 'markdown' || schema['x-pt-widget'] === 'mediainfo' || schema['x-pt-widget'] === 'textarea';
    const type = schema['x-pt-secret'] ? 'password' : schema.format === 'date' ? 'date' : schema.format === 'date-time' ? 'datetime-local' : 'text';
    return (
      <FieldWrap label={label} error={err} hint={schema.description}>
        {multiline ? (
          <textarea id={id} className={`pt-input${schema['x-pt-widget'] === 'mediainfo' ? ' pt-mono' : ''}`} rows={6} value={typeof value === 'string' ? value : ''} onChange={(e) => onChange(e.target.value)} />
        ) : (
          <input id={id} className="pt-input" type={type} maxLength={schema.maxLength} value={typeof value === 'string' ? value : ''} onChange={(e) => onChange(e.target.value === '' ? undefined : e.target.value)} />
        )}
      </FieldWrap>
    );
  }
  // 未覆盖的结构：JSON 文本编辑
  return <JsonField label={label} value={value} onChange={onChange} error={err} />;
}

/** 文本文件解码：UTF-16 LE / BE（BOM）或 UTF-8（EAC 日志常为 UTF-16 LE） */
export function decodeText(buf: ArrayBuffer): string {
  const b = new Uint8Array(buf);
  if (b[0] === 0xff && b[1] === 0xfe) return new TextDecoder('utf-16le').decode(b.subarray(2));
  if (b[0] === 0xfe && b[1] === 0xff) return new TextDecoder('utf-16be').decode(b.subarray(2));
  return new TextDecoder('utf-8').decode(b);
}

/** 日志文件：选择多个 .log / .txt，读成文本数组 */
function LogFiles({ label, error, hint, value, onChange }: { label: string; error?: string; hint?: string; value: string[]; onChange(v: unknown): void }) {
  return (
    <FieldWrap label={label} error={error} hint={hint}>
      <input
        type="file"
        multiple
        accept=".log,.txt,text/plain"
        onChange={async (e) => {
          const files = Array.from(e.target.files ?? []);
          const texts = await Promise.all(files.map(async (f) => decodeText(await f.arrayBuffer())));
          onChange(texts.length > 0 ? [...value, ...texts].slice(0, 5) : value);
          e.target.value = '';
        }}
      />
      {value.length > 0 && (
        <span className="pt-field-hint">
          {value.map((l, i) => `#${i + 1} ${(l.length / 1024).toFixed(1)} KiB`).join(' · ')}{' '}
          <button type="button" className="pt-btn pt-btn-ghost" onClick={() => onChange(undefined)}>
            ✕
          </button>
        </span>
      )}
    </FieldWrap>
  );
}

export function JsonField({ label, value, onChange, error }: { label: string; value: unknown; onChange(v: unknown): void; error?: string }) {
  const [text, setText] = useState(() => JSON.stringify(value ?? null, null, 2));
  const [bad, setBad] = useState<string | null>(null);
  return (
    <FieldWrap label={label} error={bad ?? error}>
      <textarea
        className="pt-input pt-mono"
        rows={Math.min(24, Math.max(4, text.split('\n').length))}
        value={text}
        onChange={(e) => {
          setText(e.target.value);
          try {
            onChange(JSON.parse(e.target.value));
            setBad(null);
          } catch {
            setBad('invalid JSON');
          }
        }}
      />
    </FieldWrap>
  );
}

export function SchemaForm({ schema, value, onChange, errors = {}, t = (k) => k }: { schema: Schema; value: unknown; onChange(v: unknown): void; errors?: Errors; t?: (k: string) => string }) {
  return <SchemaField schema={schema} value={value} onChange={onChange} path="" errors={errors} label={schema.title ?? ''} t={t} />;
}
