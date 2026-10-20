// 发布：选择种子文件 → 确认资源信息（类型、标题、TMDB 匹配、类型字段）→ 封面与截图 → 简介与发布。
// 桌面右侧为发布前检查与「发布」按钮（随滚动固定），手机端「发布」固定在底部（P5）。
import { useEffect, useMemo, useRef, useState, type FormEvent } from 'react';
import { useNavigate } from 'react-router';
import { api, apiFetch, parse, renderPreview, ApiError } from '@pt-studio/sdk';
import { Slot } from '@pt-studio/slots';
import { formatBytes, useT } from '@pt-studio/terms';
import { SchemaForm, serverErrors, validate, withDefaults, type Errors, type Schema } from '@pt-studio/schema-form';
import { Button, Checkbox, Field, Input, MarkdownEditor, MobileBar, PageHead, Panel, Spinner, Status, Timeline } from '@pt-studio/ui-kit';
import { ErrorText } from '../errors';
import { useInstalled } from '../manifest';
import { useTypes, type TypeInfo } from '../types';

/** 发布名 → 片名 / 年份 / 季号（`The.Last.of.Us.S02E05.1080p…` → The Last of Us、S2） */
export function parseRelease(raw: string): { title: string; year?: number; season?: number } {
  const name = raw.replace(/\.torrent$/i, '').replace(/^\[[^\]]*\]\s*/, '');
  const cut = name.search(/[.\s_](?:19|20)\d{2}(?:[.\s_]|$)|[.\s_]S\d{1,2}(?:E\d+)?(?:[.\s_]|$)|[.\s_](?:Complete|2160p|1080p|1080i|720p|480p)/i);
  const title = (cut > 0 ? name.slice(0, cut) : name).replace(/[._]+/g, ' ').replace(/\s+/g, ' ').trim();
  const y = /[.\s_]((?:19|20)\d{2})(?:[.\s_]|$)/.exec(name);
  const sn = /[.\s_]S(\d{1,2})(?:E\d+)?(?:[.\s_]|$)/i.exec(name);
  return { title, year: y ? Number(y[1]) : undefined, season: sn ? Number(sn[1]) : undefined };
}

interface MetaHit {
  external_id: string;
  title: string | null;
  original: string | null;
  year: number | null;
}

export interface MetaPick {
  hit: MetaHit;
  fields: Record<string, unknown>;
  subtitle: string;
}

const typeRank = (id: string) => {
  const i = ['movie', 'tv', 'anime', 'music', 'generic'].indexOf(id);
  return i < 0 ? 4 : i;
};

/** 按发布名自动搜 TMDB（中文名优先），可换选；选中后写入 TMDB ID（上传后自动补全海报、简介、中文名） */
function MetadataMatch({ typeId, name, onPick }: { typeId: string; name: string; onPick(p: MetaPick | null): void }) {
  const t = useT();
  const parsed = useMemo(() => parseRelease(name), [name]);
  const [q, setQ] = useState('');
  const [hits, setHits] = useState<MetaHit[]>([]);
  const [chosen, setChosen] = useState<MetaHit | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const kind = typeId === 'movie' ? 'movie' : 'tv';
  const query = q || parsed.title;
  const pick = (h: MetaHit) => {
    setChosen(h);
    const id = Number(h.external_id.split(':')[1]);
    const fields: Record<string, unknown> = kind === 'movie' ? { tmdb_id: id } : { tmdb_tv_id: id };
    if (kind === 'tv' && parsed.season) fields.season = parsed.season;
    onPick({ hit: h, fields, subtitle: h.title && h.title !== h.original ? h.title : (h.original ?? '') });
  };
  useEffect(() => {
    setChosen(null);
    onPick(null);
    if (query.length < 2) {
      setHits([]);
      return;
    }
    const h = setTimeout(async () => {
      setBusy(true);
      setError(null);
      try {
        const year = parsed.year && kind === 'movie' ? `&year=${parsed.year}` : '';
        const r = await api<{ items: MetaHit[] }>(`/api/p/metadata-tmdb/v1/search?q=${encodeURIComponent(query)}&type=${kind}${year}`);
        setHits(r.items);
        const best = r.items.find((x) => parsed.year && x.year === parsed.year) ?? (r.items.length === 1 ? r.items[0] : undefined);
        if (best) pick(best);
      } catch (e) {
        setError(e);
      } finally {
        setBusy(false);
      }
    }, 600);
    return () => clearTimeout(h);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query, kind]);
  return (
    <div className="stack-s meta-match">
      <div className="row">
        <b className="small">{t('ui.upload.match_title')}</b>
        {busy && <Spinner />}
        {chosen && <Status tone="ok">{t('ui.upload.matched')}</Status>}
      </div>
      <Input name="meta_q" placeholder={parsed.title || t('ui.upload.meta_search')} value={q} onChange={(e) => setQ(e.target.value)} />
      <ErrorText error={error} />
      <div className="matches">
        {hits.slice(0, 5).map((h) => (
          <button type="button" key={h.external_id} className="match" aria-pressed={chosen?.external_id === h.external_id} onClick={() => pick(h)}>
            <b>{h.title}</b>
            <span className="muted small">
              {h.original && h.original !== h.title && `${h.original} · `}
              {h.year ?? ''}
            </span>
          </button>
        ))}
        {!busy && hits.length === 0 && query.length >= 2 && <p className="muted small">{t('ui.upload.no_match')}</p>}
      </div>
    </div>
  );
}

/** 图片选择：缩略图预览与移除；`max` > 1 时多选 */
function ImagePicker({ label, hint, files, onChange, max = 1 }: { label: string; hint?: string; files: File[]; onChange(f: File[]): void; max?: number }) {
  const t = useT();
  const urls = useMemo(() => files.map((f) => URL.createObjectURL(f)), [files]);
  useEffect(() => () => urls.forEach((u) => URL.revokeObjectURL(u)), [urls]);
  return (
    <div className="fl">
      <span className="fl-label">{label}</span>
      <div className="shots">
        {urls.map((u, i) => (
          <div key={u} className="shot">
            <img src={u} alt="" />
            <button type="button" className="shot-x" aria-label={t('ui.common.remove')} onClick={() => onChange(files.filter((_, j) => j !== i))}>
              ×
            </button>
          </div>
        ))}
        {files.length < max && (
          <label className="shot-add">
            <input
              type="file"
              accept="image/jpeg,image/png,image/webp"
              multiple={max > 1}
              onChange={(e) => {
                const picked = [...(e.target.files ?? [])].filter((f) => f.size <= 3 << 20);
                onChange([...files, ...picked].slice(0, max));
                e.target.value = '';
              }}
            />
            <span>{t('ui.upload.add_image')}</span>
          </label>
        )}
      </div>
      {hint && <span className="hint">{hint}</span>}
    </div>
  );
}

function TypeForm({ info, value, onChange, errors }: { info: TypeInfo; value: Record<string, unknown>; onChange(v: Record<string, unknown>): void; errors: Errors }) {
  const t = useT();
  if (!info.field_schema) return null;
  return <SchemaForm schema={info.field_schema} value={value} onChange={(v) => onChange((v as Record<string, unknown>) ?? {})} errors={errors} t={t} />;
}

export function Upload() {
  const t = useT();
  const nav = useNavigate();
  const types = useTypes();
  const [typeId, setTypeId] = useState<string>('');
  const [meta, setMeta] = useState({ title: '', subtitle: '', description: '', anonymous: false });
  const [fields, setFields] = useState<Record<string, unknown>>({});
  const [file, setFile] = useState<File | null>(null);
  const [match, setMatch] = useState<MetaPick | null>(null);
  const [poster, setPoster] = useState<File[]>([]);
  const [shots, setShots] = useState<File[]>([]);
  const [error, setError] = useState<unknown>(null);
  const [errors, setErrors] = useState<Errors>({});
  const [busy, setBusy] = useState(false);
  const list = types.data ?? [];
  // 通用类型（other）放最后，默认选第一个具体类型
  const ordered = [...list].sort((x, y) => typeRank(x.type_id) - typeRank(y.type_id));
  const info = ordered.find((x) => x.type_id === typeId) ?? ordered[0];
  const hasTmdb = useInstalled('metadata-tmdb');
  const tmdbType = !!info && ['movie', 'tv', 'anime'].includes(info.type_id) && hasTmdb;
  const pick = (id: string) => {
    setTypeId(id);
    const i = list.find((x) => x.type_id === id);
    setFields(i?.field_schema ? ((withDefaults(i.field_schema as Schema, {}) as Record<string, unknown>) ?? {}) : {});
    setErrors({});
  };
  // 插件登记的发布后回调（如 mediainfo 保存粘贴的报告）；失败只提示，不影响跳转
  const afterUpload = useRef<((id: number) => Promise<void> | void)[]>([]);
  const submit = async (e?: FormEvent) => {
    e?.preventDefault();
    if (!file || !info) return;
    // 客户端预校验（发布名可补全的字段由服务端判定，这里只查类型与范围）
    const errs = info.field_schema ? Object.fromEntries(Object.entries(validate(info.field_schema, fields)).filter(([, m]) => m !== 'required')) : {};
    setErrors(errs);
    if (Object.keys(errs).length) return;
    setBusy(true);
    setError(null);
    try {
      const fd = new FormData();
      fd.append('meta', JSON.stringify({ ...meta, type_id: info.type_id, fields: { ...fields, ...(match?.fields ?? {}) }, subtitle: meta.subtitle || undefined }));
      fd.append('torrent', file, file.name);
      for (const f of poster) fd.append('poster', f, f.name);
      for (const f of shots) fd.append('screenshot', f, f.name);
      const r = await parse<{ id: number }>(await apiFetch('/api/p/torrent/v1/torrents', { method: 'POST', body: fd, idempotent: true }));
      await Promise.allSettled(afterUpload.current.map((cb) => Promise.resolve().then(() => cb(r.id))));
      nav(`/torrents/${r.id}`);
    } catch (err) {
      if (err instanceof ApiError) setErrors(serverErrors(err.details));
      setError(err);
    } finally {
      setBusy(false);
    }
  };
  const onFile = (f: File | null) => {
    setFile(f);
    if (!f) return;
    const base = f.name.replace(/\.torrent$/i, '');
    if (!meta.title) setMeta((m) => ({ ...m, title: m.title || base }));
    // 没手动选过类型时按文件名猜：有季 / 集号的是剧集，否则电影
    if (!typeId) {
      const guess = /(?:^|[.\s_])S\d{1,2}(?:E\d+)?(?:[.\s_]|$)|Complete|第.+[季集]/i.test(base) ? 'tv' : 'movie';
      if (list.some((x) => x.type_id === guess)) pick(guess);
    }
  };
  const parsed = file ? parseRelease(meta.title || file.name) : null;
  const step = file ? (meta.description ? 4 : 2) : 1;
  const ready = !!file && !!info && !busy;
  const checks = [
    { ok: !!file, text: file ? `${file.name} · ${formatBytes(file.size)}` : t('ui.upload.check_file') },
    { ok: !!info, text: info ? t(`term:${info.name_term}`) : t('ui.upload.check_type') },
    ...(tmdbType ? [{ ok: !!match, warn: !match, text: match ? `${match.hit.title ?? ''} ${match.hit.year ?? ''}` : t('ui.upload.check_tmdb') }] : []),
    { ok: poster.length > 0, warn: poster.length === 0, text: poster.length ? t('ui.upload.check_poster_ok') : t('ui.upload.check_poster') },
    { ok: !!meta.description, warn: !meta.description, text: meta.description ? t('ui.upload.check_desc_ok') : t('ui.upload.check_desc') },
  ];
  return (
    <div className="upload">
      <PageHead title={t('ui.upload.title')} lede={t('ui.upload.lede')} />
      <ol className="steps" aria-label={t('ui.upload.steps')}>
        {['step_file', 'step_info', 'step_media', 'step_more'].map((k, i) => (
          <li key={k} className={i + 1 < step ? 'done' : i + 1 === step ? 'cur' : undefined}>
            <span>{t('ui.upload.step_n', { n: i + 1 })}</span>
            <b>{t(`ui.upload.${k}`)}</b>
          </li>
        ))}
      </ol>
      <form id="upload-form" onSubmit={submit} className="side-r upload-grid">
        <div className="stack">
          <Panel title={t('ui.upload.step_file')} primary>
            <div className="stack-s">
              <label className={file ? 'drop ok' : 'drop'}>
                <input name="torrent" type="file" accept=".torrent,application/x-bittorrent" onChange={(e) => onFile(e.target.files?.[0] ?? null)} required />
                <b>{file ? file.name : t('ui.upload.file')}</b>
                <span className="small muted">{file ? formatBytes(file.size) : t('ui.upload.file_hint')}</span>
              </label>
              {parsed && (
                <div className="parsed">
                  <div>
                    <span>{t('ui.upload.parsed_title')}</span>
                    <b>{parsed.title || '-'}</b>
                  </div>
                  {parsed.year && (
                    <div>
                      <span>{t('ui.upload.parsed_year')}</span>
                      <b>{parsed.year}</b>
                    </div>
                  )}
                  {parsed.season && (
                    <div>
                      <span>{t('ui.upload.parsed_season')}</span>
                      <b>S{parsed.season}</b>
                    </div>
                  )}
                </div>
              )}
            </div>
          </Panel>
          <Panel title={t('ui.upload.step_info')}>
            <div className="form">
              <div className="fl">
                <span className="fl-label">{t('ui.torrent.type')}</span>
                <div className="chips" role="radiogroup" aria-label={t('ui.torrent.type')}>
                  {ordered.map((x) => (
                    <button key={x.type_id} type="button" role="radio" className="chip" aria-checked={info?.type_id === x.type_id} onClick={() => pick(x.type_id)}>
                      {t(`term:${x.name_term}`)}
                    </button>
                  ))}
                </div>
              </div>
              <Field label={t('ui.torrent.title')} hint={info?.plugin ? t('ui.upload.title_hint') : undefined}>
                <Input name="title" value={meta.title} onChange={(e) => setMeta({ ...meta, title: e.target.value })} required={!info?.plugin} maxLength={300} />
              </Field>
              <Field label={t('ui.upload.subtitle')}>
                <Input name="subtitle" value={meta.subtitle} onChange={(e) => setMeta({ ...meta, subtitle: e.target.value })} maxLength={300} />
              </Field>
              {tmdbType && info && (
                <MetadataMatch
                  typeId={info.type_id}
                  name={meta.title || file?.name || ''}
                  onPick={(p) => {
                    setMatch(p);
                    if (p && !meta.subtitle) setMeta((m) => ({ ...m, subtitle: m.subtitle || p.subtitle }));
                  }}
                />
              )}
              {info && (
                <details className="more-fields" open={Object.keys(errors).length > 0 || info.type_id === 'generic' || undefined}>
                  <summary>{t('ui.upload.more_fields')}</summary>
                  <div className="more-fields-b">
                    <Slot
                      name="torrent.upload.type_form"
                      ctx={{ type_id: info.type_id, schema: info.field_schema, value: fields, onChange: setFields }}
                      fallback={<TypeForm info={info} value={fields} onChange={setFields} errors={errors} />}
                      empty={<TypeForm info={info} value={fields} onChange={setFields} errors={errors} />}
                    />
                  </div>
                </details>
              )}
            </div>
          </Panel>
          <Panel title={t('ui.upload.step_media')} sub={t('ui.common.optional')}>
            <div className="form-2">
              <ImagePicker label={t('ui.upload.poster')} hint={t('ui.upload.image_hint')} files={poster} onChange={setPoster} />
              <ImagePicker label={t('ui.upload.screenshots')} hint={t('ui.upload.image_hint')} files={shots} onChange={setShots} max={6} />
            </div>
          </Panel>
          <Panel title={t('ui.upload.step_more')}>
            <div className="form">
              <Field label={t('ui.torrent.description')}>
                <MarkdownEditor name="description" rows={12} value={meta.description} onChange={(d) => setMeta({ ...meta, description: d })} preview={renderPreview} />
              </Field>
              <Slot
                name="torrent.upload.extra"
                ctx={{
                  draft: { ...meta, type_id: info?.type_id, fields },
                  // 插件回填标题 / 副标题 / 描述（如豆瓣简介）
                  update: (patch: Partial<typeof meta>) => setMeta((m) => ({ ...m, ...patch })),
                  // 发布成功后以种子 ID 回调；返回取消登记的函数
                  afterUpload: (cb: (id: number) => Promise<void> | void) => {
                    afterUpload.current.push(cb);
                    return () => {
                      afterUpload.current = afterUpload.current.filter((x) => x !== cb);
                    };
                  },
                }}
                className="stack"
              />
            </div>
          </Panel>
        </div>
        <div className="upload-side">
          <Panel title={t('ui.upload.checklist')}>
            <div className="stack-s">
              <Timeline items={checks.map((c, i) => ({ key: i, tone: c.ok ? 'ok' : c.warn ? 'warn' : undefined, children: c.text }))} />
              <Checkbox label={t('ui.upload.anonymous')} checked={meta.anonymous} onChange={(v) => setMeta({ ...meta, anonymous: v })} />
              <ErrorText error={error} />
              <Button type="submit" variant="primary" className="m-hide" disabled={!ready}>
                {busy ? <Spinner /> : t('ui.upload.submit')}
              </Button>
              <span className="tiny muted">{t('ui.upload.after_hint')}</span>
            </div>
          </Panel>
        </div>
      </form>
      <MobileBar>
        <Button type="submit" form="upload-form" variant="primary" disabled={!ready}>
          {busy ? <Spinner /> : t('ui.upload.submit')}
        </Button>
      </MobileBar>
    </div>
  );
}
