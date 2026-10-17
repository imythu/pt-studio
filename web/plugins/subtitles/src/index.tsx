// subtitles 前端：种子页“字幕”标签页（列表、下载、上传、删除）、后台最近上传。
import { useEffect, useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { formatBytes, registerMessages } from '@pt-studio/terms';
import { Badge, Button, Card, Field, Input, Table } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'subtitles.ui.tab': '字幕',
  'subtitles.ui.empty': '还没有字幕',
  'subtitles.ui.upload': '上传字幕',
  'subtitles.ui.file': '字幕文件（SRT / ASS / SSA / VTT / SUP，或 ZIP 字幕包）',
  'subtitles.ui.lang': '语言',
  'subtitles.ui.title': '说明（可选，如“官方中字”“对应 WEB-DL 时间轴”）',
  'subtitles.ui.anonymous': '匿名上传',
  'subtitles.ui.submit': '上传',
  'subtitles.ui.uploaded': '字幕已上传',
  'subtitles.ui.download': '下载',
  'subtitles.ui.delete': '删除',
  'subtitles.ui.delete_reason': '删除原因',
  'subtitles.ui.downloads': '{n} 次下载',
  'subtitles.ui.anon': '匿名',
  'subtitles.ui.admin': '最近上传的字幕',
  'subtitles.ui.files': '{n} 个文件',
  'subtitles.lang.zh-hans': '简体中文',
  'subtitles.lang.zh-hant': '繁体中文',
  'subtitles.lang.zh-hans-en': '简英双语',
  'subtitles.lang.zh-hant-en': '繁英双语',
  'subtitles.lang.en': '英语',
  'subtitles.lang.ja': '日语',
  'subtitles.lang.ko': '韩语',
  'subtitles.lang.other': '其他',
  'subtitles.too_large': '字幕文件太大',
  'subtitles.invalid_file': '字幕文件不符合要求',
  'subtitles.duplicate': '这个字幕已经上传过了',
  'subtitles.limit_reached': '这个种子的字幕数量已达上限',
});
registerMessages('en', {
  'subtitles.ui.tab': 'Subtitles',
  'subtitles.ui.empty': 'No subtitles yet',
  'subtitles.ui.upload': 'Upload subtitles',
  'subtitles.ui.file': 'Subtitle file (SRT / ASS / SSA / VTT / SUP, or a ZIP pack)',
  'subtitles.ui.lang': 'Language',
  'subtitles.ui.title': 'Note (optional, e.g. "official", "synced to WEB-DL")',
  'subtitles.ui.anonymous': 'Upload anonymously',
  'subtitles.ui.submit': 'Upload',
  'subtitles.ui.uploaded': 'Subtitle uploaded',
  'subtitles.ui.download': 'Download',
  'subtitles.ui.delete': 'Delete',
  'subtitles.ui.delete_reason': 'Reason for deletion',
  'subtitles.ui.downloads': '{n} downloads',
  'subtitles.ui.anon': 'Anonymous',
  'subtitles.ui.admin': 'Recent subtitles',
  'subtitles.ui.files': '{n} files',
  'subtitles.lang.zh-hans': 'Simplified Chinese',
  'subtitles.lang.zh-hant': 'Traditional Chinese',
  'subtitles.lang.zh-hans-en': 'Simplified Chinese + English',
  'subtitles.lang.zh-hant-en': 'Traditional Chinese + English',
  'subtitles.lang.en': 'English',
  'subtitles.lang.ja': 'Japanese',
  'subtitles.lang.ko': 'Korean',
  'subtitles.lang.other': 'Other',
  'subtitles.too_large': 'The subtitle file is too large',
  'subtitles.invalid_file': 'The subtitle file is not accepted',
  'subtitles.duplicate': 'This subtitle was already uploaded',
  'subtitles.limit_reached': 'This torrent has too many subtitles',
});

const errText = (api: PluginApi, e: unknown) => {
  if (e instanceof ApiError) {
    const base = api.t(e.code) !== e.code ? api.t(e.code) : e.message;
    return e.code === 'subtitles.invalid_file' && e.message ? `${base}：${e.message}` : base;
  }
  return e instanceof Error ? e.message : String(e);
};

interface Sub {
  id: number;
  lang: string;
  title: string;
  filename: string;
  format: string;
  encoding: string | null;
  size: number;
  downloads: number;
  uploader: string | null;
  anonymous: boolean;
  created_at: number;
  entries: { name: string; size: number }[];
  can_delete: boolean;
}

function UploadForm({ api, tid, langs, done }: { api: PluginApi; tid: number; langs: string[]; done(): void }) {
  const [file, setFile] = useState<File | null>(null);
  const [lang, setLang] = useState('zh-hans');
  const [title, setTitle] = useState('');
  const [anon, setAnon] = useState(false);
  const [busy, setBusy] = useState(false);
  const submit = async () => {
    if (!file) return;
    setBusy(true);
    try {
      const q = new URLSearchParams({ filename: file.name, lang, title, ...(anon ? { anonymous: '1' } : {}) });
      await api.json(`/api/p/subtitles/v1/torrents/${tid}/subtitles?${q.toString()}`, {
        method: 'POST',
        body: file,
        headers: { 'content-type': 'application/octet-stream' },
      });
      api.toast(api.t('subtitles.ui.uploaded'), 'success');
      setFile(null);
      setTitle('');
      done();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    } finally {
      setBusy(false);
    }
  };
  return (
    <form
      className="subtitles-upload"
      onSubmit={(e) => {
        e.preventDefault();
        void submit();
      }}
    >
      <h4>{api.t('subtitles.ui.upload')}</h4>
      <Field label={api.t('subtitles.ui.file')}>
        <input type="file" name="subtitle_file" accept=".srt,.ass,.ssa,.vtt,.sup,.zip" onChange={(e) => setFile(e.target.files?.[0] ?? null)} />
      </Field>
      <Field label={api.t('subtitles.ui.lang')}>
        <select className="pt-input" name="subtitle_lang" value={lang} onChange={(e) => setLang(e.target.value)}>
          {langs.map((l) => (
            <option key={l} value={l}>
              {api.t(`subtitles.lang.${l}`)}
            </option>
          ))}
        </select>
      </Field>
      <Field label={api.t('subtitles.ui.title')}>
        <Input name="subtitle_title" value={title} maxLength={200} onChange={(e) => setTitle(e.target.value)} />
      </Field>
      <label className="pt-check">
        <input type="checkbox" checked={anon} onChange={(e) => setAnon(e.target.checked)} /> {api.t('subtitles.ui.anonymous')}
      </label>
      <Button type="submit" variant="primary" disabled={!file || busy}>
        {api.t('subtitles.ui.submit')}
      </Button>
    </form>
  );
}

function TorrentTab({ api, ctx }: SlotProps<{ torrent: { id: number } }>) {
  const tid = ctx.torrent.id;
  const [d, setD] = useState<{ items: Sub[]; langs: string[] } | null>(null);
  const load = () =>
    api
      .json<{ items: Sub[]; langs: string[] }>(`/api/p/subtitles/v1/torrents/${tid}/subtitles`)
      .then(setD)
      .catch(() => undefined);
  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tid]);
  if (!d) return null;
  const del = async (s: Sub) => {
    // 上传者删除自己的可留空；管理员删除他人的需填写原因（服务端校验）
    const reason = window.prompt(api.t('subtitles.ui.delete_reason'));
    if (reason === null) return;
    try {
      await api.fetch(`/api/p/subtitles/v1/subtitles/${s.id}?reason=${encodeURIComponent(reason)}`, { method: 'DELETE' });
      await load();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  return (
    <div className="pt-stack">
      <Table
        rows={d.items}
        rowKey={(r) => r.id}
        empty={api.t('subtitles.ui.empty')}
        columns={[
          { key: 'l', header: api.t('subtitles.ui.lang'), render: (r) => <Badge tone="primary">{api.t(`subtitles.lang.${r.lang}`)}</Badge> },
          {
            key: 'f',
            header: '',
            render: (r) => (
              <>
                <span className="pt-mono">{r.filename}</span> <span className="pt-muted small">{r.format.toUpperCase()}</span>
                {r.entries.length > 0 && <span className="pt-muted small"> · {api.t('subtitles.ui.files', { n: r.entries.length })}</span>}
                {r.title && <div className="small">{r.title}</div>}
              </>
            ),
          },
          { key: 's', header: '', render: (r) => formatBytes(r.size) },
          {
            key: 'u',
            header: '',
            render: (r) => (
              <span className="pt-muted small">
                {r.uploader ?? api.t('subtitles.ui.anon')} · {new Date(r.created_at).toLocaleDateString()} · {api.t('subtitles.ui.downloads', { n: r.downloads })}
              </span>
            ),
          },
          {
            key: 'a',
            header: '',
            render: (r) => (
              <span>
                <a className="pt-btn" href={`/api/p/subtitles/v1/subtitles/${r.id}/download`} download onClick={() => setTimeout(() => void load(), 1000)}>
                  {api.t('subtitles.ui.download')}
                </a>
                {r.can_delete && (
                  <Button variant="ghost" onClick={() => void del(r)}>
                    {api.t('subtitles.ui.delete')}
                  </Button>
                )}
              </span>
            ),
          },
        ]}
      />
      <UploadForm api={api} tid={tid} langs={d.langs} done={() => void load()} />
    </div>
  );
}

function Admin({ api }: { api: PluginApi }) {
  const [items, setItems] = useState<{ id: number; torrent_id: number; torrent_title: string | null; lang: string; filename: string; size: number; downloads: number; uploader: string | null }[]>([]);
  useEffect(() => {
    void api.json<{ items: typeof items }>('/api/p/subtitles/v1/admin/subtitles').then((r) => setItems(r.items));
  }, [api]);
  return (
    <Card title={api.t('subtitles.ui.admin')}>
      <Table
        rows={items}
        rowKey={(r) => r.id}
        empty={api.t('subtitles.ui.empty')}
        columns={[
          {
            key: 't',
            header: '',
            render: (r) => (
              <a
                href={`/torrents/${r.torrent_id}`}
                onClick={(e) => {
                  e.preventDefault();
                  api.navigate(`/torrents/${r.torrent_id}`);
                }}
              >
                {r.torrent_title ?? `#${r.torrent_id}`}
              </a>
            ),
          },
          { key: 'f', header: '', render: (r) => `${api.t(`subtitles.lang.${r.lang}`)} · ${r.filename}` },
          { key: 's', header: '', render: (r) => formatBytes(r.size) },
          { key: 'u', header: '', render: (r) => `${r.uploader ?? '?'} · ${api.t('subtitles.ui.downloads', { n: r.downloads })}` },
        ]}
      />
    </Card>
  );
}

export default {
  slots: { 'torrent.detail.tabs': { component: TorrentTab, order: 20, title_term: 'subtitles.ui.tab' } },
  routes: [{ path: '/admin/p/subtitles/*', component: Admin }],
} satisfies PluginFrontend;
