// @pt-studio/schema-form：JSON Schema 驱动的表单（web LLD §4）。
//
// 校验器为无 eval 的子集实现（CSP 不允许 `unsafe-eval`，Ajv 需要代码生成）：type、required、enum、minimum / maximum、
// minLength / maxLength、pattern、items、properties、additionalProperties: false。

import { createContext, useContext, useEffect, useState, type KeyboardEvent, type ReactNode } from 'react';

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
  // 可空对象缺省为未启用
  if (value === undefined && Array.isArray(schema.type) && schema.type.includes('null')) return undefined;
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

/** 键序无关的深比较 */
export function same(a: unknown, b: unknown): boolean {
  return stable(a) === stable(b);
}

function stable(v: unknown): string {
  if (v === undefined) return 'undefined';
  if (v === null || typeof v !== 'object') return JSON.stringify(v);
  if (Array.isArray(v)) return `[${v.map(stable).join(',')}]`;
  return `{${Object.keys(v as object)
    .filter((k) => (v as Record<string, unknown>)[k] !== undefined)
    .sort()
    .map((k) => `${JSON.stringify(k)}:${stable((v as Record<string, unknown>)[k])}`)
    .join(',')}}`;
}

/** 按 JSON Pointer 取值 */
export function at(v: unknown, path: string): unknown {
  let cur = v;
  for (const seg of path.split('/').slice(1)) {
    if (cur === null || typeof cur !== 'object') return undefined;
    cur = (cur as Record<string, unknown>)[seg.replace(/~1/g, '/').replace(/~0/g, '~')];
  }
  return cur;
}

/** 设置表单上下文：已保存值（标记“已修改”）与“恢复默认” */
const BaselineCtx = createContext<{ baseline: unknown } | null>(null);

/** 文案：t 原样返回键时用回退值 */
function tr(t: (k: string) => string, key: string, fallback: string): string {
  const v = t(key);
  return v === key ? fallback : v;
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

/** 说明文字：术语 `<label-term>.hint` 优先，缺失时用 schema 的 description */
function hintOf(s: Schema, t: (k: string) => string): string | undefined {
  const term = s['x-pt-label-term'];
  if (term) {
    const k = `${term}.hint`;
    const v = t(`term:${k}`);
    if (v !== k) return v;
  }
  return s.description;
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

interface Meta {
  path: string;
  schema: Schema;
  value: unknown;
  onChange(v: unknown): void;
  t(k: string): string;
}

function FieldWrap({ label, error, hint, children, meta }: { label: string; error?: string; hint?: string; children: ReactNode; meta?: Meta }) {
  const ctx = useContext(BaselineCtx);
  const modified = !!ctx && !!meta && !same(meta.value, at(ctx.baseline, meta.path));
  const def = meta?.schema.default;
  const canReset = !!ctx && !!meta && def !== undefined && !same(meta.value, def);
  return (
    <div className={`fl${error ? ' invalid' : ''}${modified ? ' modified' : ''}`}>
      <span className="fl-label">
        {label}
        {modified && meta && <span className="fl-dot" title={tr(meta.t, 'ui.form.modified', 'Modified')} />}
        {canReset && meta && (
          <button type="button" className="fl-reset" title={`${tr(meta.t, 'ui.form.default', 'Default')}: ${JSON.stringify(def)}`} onClick={() => meta.onChange(def)}>
            {tr(meta.t, 'ui.form.reset_default', 'Reset to default')}
          </button>
        )}
      </span>
      {children}
      {hint && !error && <span className="hint">{hint}</span>}
      {error && <span className="err">{error}</span>}
    </div>
  );
}

/** 数值范围 / 默认值提示 */
function numHint(s: Schema, t: (k: string) => string): string | undefined {
  const parts = [hintOf(s, t)];
  if (s.minimum !== undefined && s.maximum !== undefined) parts.push(`${s.minimum} – ${s.maximum}`);
  else if (s.minimum !== undefined) parts.push(`≥ ${s.minimum}`);
  else if (s.maximum !== undefined) parts.push(`≤ ${s.maximum}`);
  const out = parts.filter(Boolean).join(' · ');
  return out || undefined;
}

/** 数字输入：本地保留文本，便于输入 “0.” “-” 等中间态 */
function NumberInput({ id, integer, schema, value, onChange }: { id: string; integer: boolean; schema: Schema; value: unknown; onChange(v: unknown): void }) {
  const [text, setText] = useState(typeof value === 'number' ? String(value) : '');
  useEffect(() => {
    const n = text.trim() === '' ? undefined : Number(text);
    if (n !== value && !(n === undefined && value === undefined)) setText(typeof value === 'number' ? String(value) : '');
  }, [value]);
  return (
    <input
      id={id}
      className="in"
      inputMode={integer ? 'numeric' : 'decimal'}
      value={text}
      onChange={(e) => {
        const v = e.target.value;
        setText(v);
        if (v.trim() === '') return onChange(undefined);
        const n = Number(v);
        if (Number.isFinite(n) && (!integer || Number.isInteger(n))) onChange(n);
      }}
      onBlur={() => {
        const n = Number(text);
        if (text.trim() !== '' && !(Number.isFinite(n) && (!integer || Number.isInteger(n)))) setText(typeof value === 'number' ? String(value) : '');
      }}
      placeholder={schema.default !== undefined ? String(schema.default) : undefined}
    />
  );
}

/** 字符串列表：回车 / 逗号添加，× 删除 */
function TagInput({ id, value, onChange, t }: { id: string; value: string[]; onChange(v: unknown): void; t(k: string): string }) {
  const [text, setText] = useState('');
  const commit = () => {
    const add = text.split(/[,\n]/).map((s) => s.trim()).filter((s) => s && !value.includes(s));
    if (add.length) onChange([...value, ...add]);
    setText('');
  };
  const key = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter' || e.key === ',') {
      e.preventDefault();
      commit();
    } else if (e.key === 'Backspace' && text === '' && value.length) onChange(value.slice(0, -1));
  };
  return (
    <div className="sf-tags">
      {value.map((v, i) => (
        <span key={`${v}-${i}`} className="tag">
          {v}
          <button type="button" aria-label="remove" onClick={() => onChange(value.filter((_, j) => j !== i))}>
            ×
          </button>
        </span>
      ))}
      <input id={id} className="sf-tags-input" value={text} onChange={(e) => setText(e.target.value)} onKeyDown={key} onBlur={commit} placeholder={tr(t, 'ui.form.tag_hint', 'Type and press Enter')} />
    </div>
  );
}

function SchemaField({ schema, value, onChange, path, errors, label, t }: FieldProps) {
  const err = errors[path];
  const ty = primary(schema);
  const id = `sf${path.replace(/\W/g, '_')}`;
  const m: Meta = { path, schema, value, onChange, t };
  if (schema.oneOf && discriminator(schema)) {
    const d = discriminator(schema) as string;
    const branch = branchOf(schema, value);
    const sel = schema.properties?.[d] ?? { type: 'string', enum: schema.oneOf.map((b) => b.properties?.[d]?.const) };
    const obj = (value as Record<string, unknown>) ?? {};
    return (
      <div className="sf-oneof">
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
  if (ty === 'object' && typeof schema.additionalProperties === 'object') {
    return <MapEditor schema={schema} item={schema.additionalProperties} value={value} onChange={onChange} path={path} errors={errors} label={label} t={t} />;
  }
  // 可空对象（`type = ["object", "null"]`）：开关控制启用，关闭时为 null
  if (ty === 'object' && schema.properties && Array.isArray(schema.type) && schema.type.includes('null') && path) {
    const on = value !== null && value !== undefined;
    return (
      <div className="sf-nullable">
        <FieldWrap label={label} error={err} hint={hintOf(schema, t)} meta={m}>
          <label className="sf-switch">
            <input id={id} type="checkbox" checked={on} onChange={(e) => onChange(e.target.checked ? (withDefaults({ ...schema, type: 'object' }, {}) ?? {}) : null)} />
            <span className="sf-switch-track" aria-hidden />
          </label>
        </FieldWrap>
        {on && <SchemaField schema={{ ...schema, type: 'object' }} value={value} onChange={onChange} path={path} errors={errors} label="" t={t} />}
      </div>
    );
  }
  if (ty === 'object' && schema.properties) {
    return (
      <fieldset className="sf-fieldset">
        {path && label && <legend>{label}</legend>}
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
      <FieldWrap label={label} error={err} hint={hintOf(schema, t)} meta={m}>
        <label className="sf-switch">
          <input id={id} type="checkbox" checked={value === true} onChange={(e) => onChange(e.target.checked)} />
          <span className="sf-switch-track" aria-hidden />
        </label>
      </FieldWrap>
    );
  }
  if (schema.enum) {
    return (
      <FieldWrap label={label} error={err} hint={hintOf(schema, t)} meta={m}>
        <select id={id} className="sel" value={value === undefined ? '' : JSON.stringify(value)} onChange={(e) => onChange(e.target.value === '' ? undefined : JSON.parse(e.target.value))}>
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
      <FieldWrap label={label} error={err} hint={hintOf(schema, t)} meta={m}>
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
    return <LogFiles label={label} error={err} hint={hintOf(schema, t)} value={Array.isArray(value) ? (value as string[]) : []} onChange={onChange} />;
  }
  if (ty === 'array' && (!schema.items || primary(schema.items) === 'string')) {
    const cur = Array.isArray(value) ? (value as string[]) : [];
    return (
      <FieldWrap label={label} error={err} hint={hintOf(schema, t)} meta={m}>
        <TagInput id={id} value={cur} onChange={onChange} t={t} />
      </FieldWrap>
    );
  }
  if (ty === 'array' && schema.items && primary(schema.items) === 'object') {
    return <ListEditor schema={schema} item={schema.items} value={value} onChange={onChange} path={path} errors={errors} label={label} t={t} />;
  }
  if (ty === 'integer' || ty === 'number') {
    return (
      <FieldWrap label={label} error={err} hint={numHint(schema, t)} meta={m}>
        <NumberInput id={id} integer={ty === 'integer'} schema={schema} value={value} onChange={onChange} />
      </FieldWrap>
    );
  }
  if (ty === 'string') {
    const multiline = schema['x-pt-widget'] === 'markdown' || schema['x-pt-widget'] === 'mediainfo' || schema['x-pt-widget'] === 'textarea';
    const type = schema['x-pt-secret'] ? 'password' : schema.format === 'date' ? 'date' : schema.format === 'date-time' ? 'datetime-local' : 'text';
    return (
      <FieldWrap label={label} error={err} hint={hintOf(schema, t)} meta={m}>
        {multiline ? (
          <textarea id={id} className={`ta${schema['x-pt-widget'] === 'mediainfo' ? ' code' : ''}`} rows={6} value={typeof value === 'string' ? value : ''} onChange={(e) => onChange(e.target.value)} />
        ) : (
          <input id={id} className="in" type={type} maxLength={schema.maxLength} value={typeof value === 'string' ? value : ''} onChange={(e) => onChange(e.target.value === '' ? undefined : e.target.value)} />
        )}
      </FieldWrap>
    );
  }
  // 未覆盖的结构：JSON 文本编辑
  return <JsonField label={label} value={value} onChange={onChange} error={err} />;
}

/** 对象列表：每项一张小卡片，可增删、上下移动 */
function ListEditor({ schema, item, value, onChange, path, errors, label, t }: Omit<FieldProps, 'schema'> & { schema: Schema; item: Schema }) {
  const cur = Array.isArray(value) ? value : [];
  const set = (i: number, v: unknown) => onChange(cur.map((x, j) => (j === i ? v : x)));
  const move = (i: number, d: number) => {
    const next = [...cur];
    const [x] = next.splice(i, 1);
    next.splice(i + d, 0, x);
    onChange(next);
  };
  const err = errors[path];
  return (
    <FieldWrap label={label} error={err} hint={hintOf(schema, t)} meta={{ path, schema, value, onChange, t }}>
      <div className="sf-list">
        {cur.map((x, i) => (
          <div key={i} className="sf-list-item">
            <div className="sf-list-head">
              <span className="pt-muted">#{i + 1}</span>
              <span className="sf-list-actions">
                <button type="button" className="btn btn-xs btn-ghost" disabled={i === 0} onClick={() => move(i, -1)} aria-label="up">↑</button>
                <button type="button" className="btn btn-xs btn-ghost" disabled={i === cur.length - 1} onClick={() => move(i, 1)} aria-label="down">↓</button>
                <button type="button" className="btn btn-xs btn-ghost btn-danger" onClick={() => onChange(cur.filter((_, j) => j !== i))}>
                  {tr(t, 'ui.form.remove', 'Remove')}
                </button>
              </span>
            </div>
            <SchemaField schema={item} value={x} onChange={(v) => set(i, v)} path={`${path}/${i}`} errors={errors} label="" t={t} />
          </div>
        ))}
        {cur.length === 0 && <div className="muted sf-list-empty">{tr(t, 'ui.form.empty_list', 'Nothing here yet')}</div>}
        <div>
          <button type="button" className="btn btn-sm" onClick={() => onChange([...cur, withDefaults(item, undefined) ?? (primary(item) === 'object' ? {} : undefined)])}>
            + {tr(t, 'ui.form.add', 'Add')}
          </button>
        </div>
      </div>
    </FieldWrap>
  );
}

/** 键值映射（`additionalProperties: {schema}`）：如按等级配置的规则；固定属性照常显示在前 */
function MapEditor({ schema, item, value, onChange, path, errors, label, t }: Omit<FieldProps, 'schema'> & { schema: Schema; item: Schema }) {
  const obj = (value && typeof value === 'object' && !Array.isArray(value) ? value : {}) as Record<string, unknown>;
  const fixed = schema.properties ?? {};
  const keys = Object.keys(obj).filter((k) => !(k in fixed));
  const [nk, setNk] = useState('');
  const add = () => {
    const k = nk.trim();
    if (!k || k in obj) return;
    onChange({ ...obj, [k]: withDefaults(item, undefined) ?? (primary(item) === 'object' ? {} : undefined) });
    setNk('');
  };
  const rename = (from: string, to: string) => {
    if (!to || to === from || to in obj) return;
    onChange(Object.fromEntries(Object.entries(obj).map(([k, v]) => [k === from ? to : k, v])));
  };
  return (
    <FieldWrap label={label} error={errors[path]} hint={hintOf(schema, t)} meta={{ path, schema, value, onChange, t }}>
      <div className="sf-list">
        {Object.keys(fixed).length > 0 && (
          <SchemaField schema={{ ...schema, additionalProperties: undefined }} value={obj} onChange={onChange} path={path} errors={errors} label="" t={t} />
        )}
        {keys.map((k) => (
          <div key={k} className="sf-list-item">
            <div className="sf-list-head">
              <input className="in code sf-map-key" defaultValue={k} onBlur={(e) => rename(k, e.target.value.trim())} aria-label="key" />
              <button type="button" className="btn btn-xs btn-ghost btn-danger" onClick={() => onChange(Object.fromEntries(Object.entries(obj).filter(([x]) => x !== k)))}>
                {tr(t, 'ui.form.remove', 'Remove')}
              </button>
            </div>
            <SchemaField schema={item} value={obj[k]} onChange={(v) => onChange({ ...obj, [k]: v })} path={`${path}/${k}`} errors={errors} label="" t={t} />
          </div>
        ))}
        <div className="sf-map-add">
          <input className="in code" value={nk} placeholder={tr(t, 'ui.form.new_key', 'New key')} onChange={(e) => setNk(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && (e.preventDefault(), add())} />
          <button type="button" className="btn btn-sm" onClick={add} disabled={!nk.trim() || nk.trim() in obj}>
            + {tr(t, 'ui.form.add', 'Add')}
          </button>
        </div>
      </div>
    </FieldWrap>
  );
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
        <span className="hint">
          {value.map((l, i) => `#${i + 1} ${(l.length / 1024).toFixed(1)} KiB`).join(' · ')}{' '}
          <button type="button" className="btn btn-xs btn-ghost" onClick={() => onChange(undefined)}>
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
  // 外部改动（排序、恢复默认、放弃修改）时同步文本；正在输入的无效 JSON 保留
  useEffect(() => {
    let cur: unknown;
    try {
      cur = JSON.parse(text);
    } catch {
      return;
    }
    if (!same(cur, value ?? null)) {
      setText(JSON.stringify(value ?? null, null, 2));
      setBad(null);
    }
  }, [value]);
  return (
    <FieldWrap label={label} error={bad ?? error}>
      <textarea
        className="in code"
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

/** `baseline`：已保存的值；给出时字段显示“已修改”标记与“恢复默认”（后台设置页） */
export function SchemaForm({
  schema,
  value,
  onChange,
  errors = {},
  t = (k) => k,
  baseline,
}: {
  schema: Schema;
  value: unknown;
  onChange(v: unknown): void;
  errors?: Errors;
  t?: (k: string) => string;
  baseline?: unknown;
}) {
  const form = <SchemaField schema={schema} value={value} onChange={onChange} path="" errors={errors} label={schema.title ?? ''} t={t} />;
  return baseline === undefined ? form : <BaselineCtx.Provider value={{ baseline }}>{form}</BaselineCtx.Provider>;
}
