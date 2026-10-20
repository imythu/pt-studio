// rss 前端：个人设置“RSS”标签页——feed key（生成 / 重置 / 停用、最近使用）、订阅列表（抽屉里新建 / 编辑：类型、关键词、排除词、促销）、
// 订阅地址与 Torznab 地址（Prowlarr 配置说明）。
import { useCallback, useEffect, useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Button, CodeBox, Field, Input, Note, Sheet, Table } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'rss.ui.tab': 'RSS',
  'rss.ui.key': '订阅密钥',
  'rss.ui.key_warning': '密钥可以以你的身份下载种子，不要分享给他人。重置后旧密钥立即失效，已配置的客户端需要更新。',
  'rss.ui.key_none': '还没有生成密钥',
  'rss.ui.key_current': '当前密钥：{prefix}…（只显示前 8 位）',
  'rss.ui.key_shown_once': '密钥只显示这一次，请立即保存',
  'rss.ui.generate': '生成密钥',
  'rss.ui.reset': '重置密钥',
  'rss.ui.revoke': '停用密钥',
  'rss.ui.last_used': '最近使用：{at}（{ua}）',
  'rss.ui.never': '从未使用',
  'rss.ui.feeds': '我的订阅',
  'rss.ui.name': '名称',
  'rss.ui.types': '资源类型（逗号分隔，留空为全部）',
  'rss.ui.q': '关键词',
  'rss.ui.exclude': '排除词（逗号分隔）',
  'rss.ui.promotion': '只要促销（如 free，逗号分隔）',
  'rss.ui.url': '订阅地址',
  'rss.ui.add': '添加订阅',
  'rss.ui.save': '保存',
  'rss.ui.edit': '编辑',
  'rss.ui.delete': '删除',
  'rss.ui.torznab': 'Torznab（Prowlarr / Jackett / Sonarr）',
  'rss.ui.torznab_hint': '在 Prowlarr 中添加 “Generic Torznab” 索引器：URL 填下面的地址，API Key 填订阅密钥。',
  'rss.key_invalid': '订阅密钥无效',
  'rss.too_many_feeds': '订阅数量已达上限',
  'rss.filter_invalid': '订阅条件不正确',
  'rss.feed_not_found': '订阅不存在',
  'rss.ui.cancel': '取消',
  'rss.ui.new_feed': '新建订阅',
  'rss.ui.edit_feed': '编辑订阅',
  'rss.ui.no_feeds': '还没有订阅。新建一个，把地址填进下载器的 RSS 即可自动下载。',
  'rss.ui.revoke_confirm': '停用后所有使用这个密钥的订阅和 Torznab 都会失效。',
  'rss.ui.url_hint': '完整地址只在刚生成密钥时可见，其余时候以 <key> 占位。',
});
registerMessages('en', {
  'rss.ui.tab': 'RSS',
  'rss.ui.key': 'Feed key',
  'rss.ui.key_warning': 'The key downloads torrents on your account — do not share it. Resetting invalidates the old key immediately; update your clients afterwards.',
  'rss.ui.key_none': 'No key yet',
  'rss.ui.key_current': 'Current key: {prefix}… (first 8 characters)',
  'rss.ui.key_shown_once': 'The key is shown only once — save it now',
  'rss.ui.generate': 'Generate key',
  'rss.ui.reset': 'Reset key',
  'rss.ui.revoke': 'Revoke key',
  'rss.ui.last_used': 'Last used: {at} ({ua})',
  'rss.ui.never': 'Never used',
  'rss.ui.feeds': 'My feeds',
  'rss.ui.name': 'Name',
  'rss.ui.types': 'Content types (comma separated, empty = all)',
  'rss.ui.q': 'Keywords',
  'rss.ui.exclude': 'Exclude words (comma separated)',
  'rss.ui.promotion': 'Promotions only (e.g. free, comma separated)',
  'rss.ui.url': 'Feed URL',
  'rss.ui.add': 'Add feed',
  'rss.ui.save': 'Save',
  'rss.ui.edit': 'Edit',
  'rss.ui.delete': 'Delete',
  'rss.ui.torznab': 'Torznab (Prowlarr / Jackett / Sonarr)',
  'rss.ui.torznab_hint': 'Add a “Generic Torznab” indexer in Prowlarr with the URL below and your feed key as the API key.',
  'rss.key_invalid': 'Invalid feed key',
  'rss.too_many_feeds': 'Too many feeds',
  'rss.filter_invalid': 'Invalid feed conditions',
  'rss.feed_not_found': 'Feed not found',
  'rss.ui.cancel': 'Cancel',
  'rss.ui.new_feed': 'New feed',
  'rss.ui.edit_feed': 'Edit feed',
  'rss.ui.no_feeds': 'No feeds yet. Create one and add its URL to your client’s RSS to download automatically.',
  'rss.ui.revoke_confirm': 'Every feed and Torznab client using this key will stop working.',
  'rss.ui.url_hint': 'The full URL is only visible right after generating a key; otherwise <key> is a placeholder.',
});

interface KeyInfo {
  exists: boolean;
  prefix?: string;
  last_used_at?: number | null;
  last_user_agent?: string | null;
}

interface Definition {
  type_ids?: string[];
  q?: string;
  exclude?: string[];
  promotion?: string[];
  filters?: unknown[];
  min_size?: number;
  max_size?: number;
}

interface Feed {
  id: number;
  name: string;
  definition: Definition;
  url_template: string;
}

interface Draft {
  id: number | null;
  name: string;
  types: string;
  q: string;
  exclude: string;
  promotion: string;
  rest: Definition;
}

const EMPTY: Draft = { id: null, name: '', types: '', q: '', exclude: '', promotion: '', rest: {} };

function errText(api: PluginApi, e: unknown): string {
  if (e instanceof ApiError) {
    const s = api.t(e.code);
    return s === e.code ? e.message : s;
  }
  return String(e);
}

const list = (s: string) =>
  s
    .split(/[,，]/)
    .map((x) => x.trim())
    .filter(Boolean);

function toDraft(f: Feed): Draft {
  const { type_ids, q, exclude, promotion, ...rest } = f.definition;
  return {
    id: f.id,
    name: f.name,
    types: (type_ids ?? []).join(', '),
    q: q ?? '',
    exclude: (exclude ?? []).join(', '),
    promotion: (promotion ?? []).join(', '),
    rest,
  };
}

/** 订阅条件摘要：类型 / 关键词 / 排除 / 促销 */
function summary(d: Definition): string {
  return [d.type_ids?.join('/'), d.q, d.exclude?.length ? `−${d.exclude.join(' −')}` : '', d.promotion?.join('/')].filter(Boolean).join(' · ') || '*';
}

export function SettingsTab({ api }: SlotProps) {
  const [key, setKey] = useState<KeyInfo>({ exists: false });
  const [secret, setSecret] = useState<string | null>(null);
  const [feeds, setFeeds] = useState<Feed[]>([]);
  const [torznab, setTorznab] = useState('');
  const [draft, setDraft] = useState<Draft | null>(null);
  const [revoking, setRevoking] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const load = useCallback(async () => {
    const [k, f] = await Promise.all([
      api.json<KeyInfo>('/api/p/rss/v1/me/key'),
      api.json<{ items: Feed[]; torznab_url: string }>('/api/p/rss/v1/me/feeds'),
    ]);
    setKey(k);
    setFeeds(f.items);
    setTorznab(f.torznab_url);
  }, [api]);
  useEffect(() => {
    void load().catch((e) => setErr(errText(api, e)));
  }, [api, load]);
  async function act(fn: () => Promise<void>) {
    setErr(null);
    try {
      await fn();
      await load();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  }
  const resetKey = () =>
    act(async () => {
      const r = await api.json<{ key: string }>('/api/p/rss/v1/me/key', { method: 'POST', json: {} });
      setSecret(r.key);
    });
  const revokeKey = () =>
    act(async () => {
      await api.fetch('/api/p/rss/v1/me/key', { method: 'DELETE' });
      setSecret(null);
      setRevoking(false);
    });
  const save = () =>
    draft &&
    act(async () => {
      const definition: Definition = {
        ...draft.rest,
        type_ids: list(draft.types),
        q: draft.q.trim(),
        exclude: list(draft.exclude),
        promotion: list(draft.promotion),
      };
      const json = { name: draft.name.trim(), definition };
      if (draft.id === null) await api.json('/api/p/rss/v1/me/feeds', { method: 'POST', json });
      else await api.json(`/api/p/rss/v1/me/feeds/${draft.id}`, { method: 'PUT', json });
      setDraft(null);
    });
  const remove = (id: number) => act(async () => void (await api.fetch(`/api/p/rss/v1/me/feeds/${id}`, { method: 'DELETE' })));
  // 只有刚生成的密钥能拼出完整地址；之后显示占位符
  const url = (f: Feed) => f.url_template.replace('{key}', secret ?? '<key>');
  const set = (k: keyof Draft) => (e: { target: { value: string } }) => draft && setDraft({ ...draft, [k]: e.target.value });
  return (
    <div className="stack">
      {err && <Note tone="bad">{err}</Note>}
      <div className="stack-s">
        <h3>{api.t('rss.ui.key')}</h3>
        <p className="small muted">{api.t('rss.ui.key_warning')}</p>
        <div className="set-row">
          <div>
            <b>{key.exists ? api.t('rss.ui.key_current', { prefix: key.prefix ?? '' }) : api.t('rss.ui.key_none')}</b>
            {key.exists && (
              <p>
                {key.last_used_at ? api.t('rss.ui.last_used', { at: new Date(key.last_used_at).toLocaleString(), ua: key.last_user_agent || '—' }) : api.t('rss.ui.never')}
              </p>
            )}
          </div>
          <div className="form-acts">
            <Button size="sm" variant="primary" onClick={() => void resetKey()}>
              {api.t(key.exists ? 'rss.ui.reset' : 'rss.ui.generate')}
            </Button>
            {key.exists && (
              <Button size="sm" variant="ghost" onClick={() => setRevoking(true)}>
                {api.t('rss.ui.revoke')}
              </Button>
            )}
          </div>
        </div>
        {secret && (
          <>
            <Note tone="warn">{api.t('rss.ui.key_shown_once')}</Note>
            <CodeBox copy={secret}>
              <span data-testid="rss-key">{secret}</span>
            </CodeBox>
          </>
        )}
      </div>
      <div className="stack-s">
        <div className="row-sb">
          <h3>{api.t('rss.ui.feeds')}</h3>
          <Button size="sm" onClick={() => setDraft(EMPTY)}>
            {api.t('rss.ui.new_feed')}
          </Button>
        </div>
        {feeds.length === 0 ? (
          <p className="small muted">{api.t('rss.ui.no_feeds')}</p>
        ) : (
          <div className="panel">
            <Table
              rows={feeds}
              rowKey={(r) => String(r.id)}
              columns={[
                {
                  key: 'n',
                  header: api.t('rss.ui.name'),
                  render: (r) => (
                    <>
                      <b>{r.name}</b>
                      <div className="tiny muted">{summary(r.definition)}</div>
                    </>
                  ),
                },
                { key: 'u', header: api.t('rss.ui.url'), render: (r) => <CodeBox copy={secret ? url(r) : undefined}>{url(r)}</CodeBox> },
                {
                  key: 'x',
                  header: '',
                  right: true,
                  render: (r) => (
                    <div className="form-acts">
                      <Button size="sm" variant="ghost" onClick={() => setDraft(toDraft(r))}>
                        {api.t('rss.ui.edit')}
                      </Button>
                      <Button size="sm" variant="ghost" onClick={() => void remove(r.id)}>
                        {api.t('rss.ui.delete')}
                      </Button>
                    </div>
                  ),
                },
              ]}
            />
          </div>
        )}
        {!secret && feeds.length > 0 && <p className="tiny muted">{api.t('rss.ui.url_hint')}</p>}
      </div>
      <div className="stack-s">
        <h3>{api.t('rss.ui.torznab')}</h3>
        <p className="small muted">{api.t('rss.ui.torznab_hint')}</p>
        {torznab && <CodeBox copy={torznab}>{torznab}</CodeBox>}
      </div>
      <Sheet
        open={draft !== null}
        onClose={() => setDraft(null)}
        title={api.t(draft?.id == null ? 'rss.ui.new_feed' : 'rss.ui.edit_feed')}
        footer={
          <>
            <Button variant="ghost" onClick={() => setDraft(null)}>
              {api.t('rss.ui.cancel')}
            </Button>
            <Button variant="primary" disabled={!draft?.name.trim()} onClick={() => void save()}>
              {api.t(draft?.id == null ? 'rss.ui.add' : 'rss.ui.save')}
            </Button>
          </>
        }
      >
        {draft && (
          <div className="form">
            <Field label={api.t('rss.ui.name')}>
              <Input name="rss_name" value={draft.name} maxLength={60} onChange={set('name')} />
            </Field>
            <Field label={api.t('rss.ui.types')}>
              <Input name="rss_types" value={draft.types} onChange={set('types')} />
            </Field>
            <Field label={api.t('rss.ui.q')}>
              <Input name="rss_q" value={draft.q} maxLength={200} onChange={set('q')} />
            </Field>
            <Field label={api.t('rss.ui.exclude')}>
              <Input name="rss_exclude" value={draft.exclude} onChange={set('exclude')} />
            </Field>
            <Field label={api.t('rss.ui.promotion')}>
              <Input name="rss_promotion" value={draft.promotion} onChange={set('promotion')} />
            </Field>
          </div>
        )}
      </Sheet>
      <Sheet
        open={revoking}
        onClose={() => setRevoking(false)}
        title={api.t('rss.ui.revoke')}
        footer={
          <>
            <Button variant="ghost" onClick={() => setRevoking(false)}>
              {api.t('rss.ui.cancel')}
            </Button>
            <Button variant="danger" onClick={() => void revokeKey()}>
              {api.t('rss.ui.revoke')}
            </Button>
          </>
        }
      >
        <p>{api.t('rss.ui.revoke_confirm')}</p>
      </Sheet>
    </div>
  );
}

export default {
  slots: {
    'user.settings.tabs': { component: SettingsTab, order: 50, title_term: 'rss.ui.tab' },
  },
} satisfies PluginFrontend;
