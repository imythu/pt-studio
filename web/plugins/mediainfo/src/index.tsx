// mediainfo 前端：发布页粘贴 MediaInfo / BDInfo（实时预览规格与字段差异，发布成功后保存），种子页规格卡片（发种者 / 管理员可编辑）。
import { useEffect, useRef, useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Badge, Button, Card, Field } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'mediainfo.ui.title': 'MediaInfo',
  'mediainfo.ui.paste': 'MediaInfo / BDInfo（可选，粘贴完整报告；目录与用户名会被去除）',
  'mediainfo.ui.preview': '检测到',
  'mediainfo.ui.diffs': '与填写的规格不一致',
  'mediainfo.ui.declared': '填写',
  'mediainfo.ui.detected': '检测',
  'mediainfo.ui.edit': '编辑',
  'mediainfo.ui.add': '添加 MediaInfo',
  'mediainfo.ui.save': '保存',
  'mediainfo.ui.cancel': '取消',
  'mediainfo.ui.remove': '删除',
  'mediainfo.ui.saved': 'MediaInfo 已保存',
  'mediainfo.ui.save_failed': 'MediaInfo 保存失败，可在种子页重新添加',
  'mediainfo.ui.show_text': '查看完整报告',
  'mediainfo.ui.audio': '音轨',
  'mediainfo.ui.subs': '字幕',
  'mediainfo.ui.duration': '时长',
  'mediainfo.too_large': '报告超过 256 KiB',
  'mediainfo.unrecognized': '不是 MediaInfo 或 BDInfo 报告',
});
registerMessages('en', {
  'mediainfo.ui.title': 'MediaInfo',
  'mediainfo.ui.paste': 'MediaInfo / BDInfo (optional, paste the full report; folders and user names are removed)',
  'mediainfo.ui.preview': 'Detected',
  'mediainfo.ui.diffs': 'Differs from the specs you entered',
  'mediainfo.ui.declared': 'entered',
  'mediainfo.ui.detected': 'detected',
  'mediainfo.ui.edit': 'Edit',
  'mediainfo.ui.add': 'Add MediaInfo',
  'mediainfo.ui.save': 'Save',
  'mediainfo.ui.cancel': 'Cancel',
  'mediainfo.ui.remove': 'Remove',
  'mediainfo.ui.saved': 'MediaInfo saved',
  'mediainfo.ui.save_failed': 'Saving the MediaInfo failed; you can add it again on the torrent page',
  'mediainfo.ui.show_text': 'Full report',
  'mediainfo.ui.audio': 'Audio',
  'mediainfo.ui.subs': 'Subtitles',
  'mediainfo.ui.duration': 'Duration',
  'mediainfo.too_large': 'The report is larger than 256 KiB',
  'mediainfo.unrecognized': 'Not a MediaInfo or BDInfo report',
});

const errText = (api: PluginApi, e: unknown) => (e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e));

interface Summary {
  container?: string | null;
  resolution?: string | null;
  video_codec?: string | null;
  hdr?: string[];
  bit_depth?: number | null;
  frame_rate?: string | null;
  duration_secs?: number | null;
  audio?: { codecs: string[]; format?: string | null; channels?: string | null; language?: string | null }[];
  subtitle_languages?: string[];
}
interface Diff {
  field: string;
  declared: string;
  detected: string;
}
interface Parsed {
  format: string;
  summary: Summary;
  diffs: Diff[];
  text?: string | null;
}

const dur = (s?: number | null) => (s ? `${Math.floor(s / 3600)}:${String(Math.floor((s % 3600) / 60)).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}` : null);

function SummaryView({ api, p }: { api: PluginApi; p: Parsed }) {
  const s = p.summary;
  const chips = [s.container, s.resolution, s.video_codec, ...(s.hdr ?? []), s.bit_depth ? `${s.bit_depth}bit` : null, s.frame_rate ? `${s.frame_rate}fps` : null].filter(Boolean) as string[];
  return (
    <div className="mediainfo-summary">
      <p>
        {chips.map((c) => (
          <Badge key={c} tone="primary">
            {c.toUpperCase()}
          </Badge>
        ))}{' '}
        {dur(s.duration_secs) && <span className="pt-muted small">{`${api.t('mediainfo.ui.duration')} ${dur(s.duration_secs)}`}</span>}
      </p>
      {(s.audio ?? []).length > 0 && (
        <p className="small">
          {api.t('mediainfo.ui.audio')}：{(s.audio ?? []).map((a) => [a.language, a.format, a.channels].filter(Boolean).join(' ')).join(' / ')}
        </p>
      )}
      {(s.subtitle_languages ?? []).length > 0 && (
        <p className="small">
          {api.t('mediainfo.ui.subs')}：{(s.subtitle_languages ?? []).join(' / ')}
        </p>
      )}
      {p.diffs.length > 0 && (
        <div className="pt-field-error small" role="alert">
          {api.t('mediainfo.ui.diffs')}：
          {p.diffs.map((d) => `${d.field}（${api.t('mediainfo.ui.declared')} ${d.declared}，${api.t('mediainfo.ui.detected')} ${d.detected}）`).join('；')}
        </div>
      )}
    </div>
  );
}

type UploadCtx = { draft: { fields?: Record<string, unknown> }; afterUpload?: (cb: (id: number) => Promise<void>) => () => void };

function UploadBox({ api, ctx }: SlotProps<UploadCtx>) {
  const [text, setText] = useState('');
  const [p, setP] = useState<Parsed | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const textRef = useRef('');
  textRef.current = text;
  const fields = JSON.stringify(ctx.draft.fields ?? {});
  const register = ctx.afterUpload;
  useEffect(() => {
    if (!register) return;
    return register(async (id) => {
      const t = textRef.current.trim();
      if (!t) return;
      try {
        await api.json(`/api/p/mediainfo/v1/torrents/${id}`, { method: 'PUT', json: { text: t } });
      } catch {
        api.toast(api.t('mediainfo.ui.save_failed'), 'error');
      }
    });
    // 只登记一次；回调读取最新文本
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  useEffect(() => {
    const t = text.trim();
    if (!t) {
      setP(null);
      setErr(null);
      return;
    }
    const h = setTimeout(() => {
      api
        .json<Parsed>('/api/p/mediainfo/v1/parse', { method: 'POST', json: { text: t, fields: JSON.parse(fields) } })
        .then((r) => {
          setP(r);
          setErr(null);
        })
        .catch((e) => {
          setP(null);
          setErr(errText(api, e));
        });
    }, 400);
    return () => clearTimeout(h);
  }, [text, fields, api]);
  if (!register) return null;
  return (
    <Field label={api.t('mediainfo.ui.paste')}>
      <textarea className="pt-input pt-mono" name="mediainfo" rows={6} value={text} onChange={(e) => setText(e.target.value)} />
      {err && <div className="pt-field-error">{err}</div>}
      {p && (
        <div>
          <span className="pt-muted small">{`${api.t('mediainfo.ui.preview')}（${p.format}）`}</span>
          <SummaryView api={api} p={p} />
        </div>
      )}
    </Field>
  );
}

function DetailCard({ api, ctx }: SlotProps<{ torrent: { id: number } }>) {
  const id = ctx.torrent.id;
  const [d, setD] = useState<{ result: Parsed | null; editable: boolean } | null>(null);
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState('');
  const [busy, setBusy] = useState(false);
  const load = () =>
    api
      .json<{ result: Parsed | null; editable: boolean }>(`/api/p/mediainfo/v1/torrents/${id}`)
      .then(setD)
      .catch(() => setD(null));
  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);
  if (!d || (!d.result && !d.editable)) return null;
  const save = async () => {
    setBusy(true);
    try {
      await api.json(`/api/p/mediainfo/v1/torrents/${id}`, { method: 'PUT', json: { text } });
      api.toast(api.t('mediainfo.ui.saved'), 'success');
      setEditing(false);
      await load();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    } finally {
      setBusy(false);
    }
  };
  const remove = async () => {
    try {
      await api.json(`/api/p/mediainfo/v1/torrents/${id}`, { method: 'DELETE' });
      await load();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  const actions = d.editable && !editing && (
    <span>
      <Button
        variant="ghost"
        onClick={() => {
          setText(d.result?.text ?? '');
          setEditing(true);
        }}
      >
        {api.t(d.result ? 'mediainfo.ui.edit' : 'mediainfo.ui.add')}
      </Button>
      {d.result && (
        <Button variant="ghost" onClick={() => void remove()}>
          {api.t('mediainfo.ui.remove')}
        </Button>
      )}
    </span>
  );
  return (
    <Card title={api.t('mediainfo.ui.title')} actions={actions || null}>
      {editing ? (
        <div>
          <textarea className="pt-input pt-mono" name="mediainfo_edit" rows={12} value={text} onChange={(e) => setText(e.target.value)} />
          <p>
            <Button variant="primary" disabled={busy || !text.trim()} onClick={() => void save()}>
              {api.t('mediainfo.ui.save')}
            </Button>{' '}
            <Button variant="ghost" onClick={() => setEditing(false)}>
              {api.t('mediainfo.ui.cancel')}
            </Button>
          </p>
        </div>
      ) : (
        d.result && (
          <>
            <SummaryView api={api} p={d.result} />
            <details>
              <summary>{api.t('mediainfo.ui.show_text')}</summary>
              <pre className="pt-mono small mediainfo-text">{d.result.text}</pre>
            </details>
          </>
        )
      )}
    </Card>
  );
}

export default {
  slots: {
    'torrent.upload.extra': { component: UploadBox, order: 10 },
    'torrent.detail.info': { component: DetailCard, order: 50 },
  },
} satisfies PluginFrontend;
