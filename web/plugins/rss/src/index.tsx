// rss 前端：个人设置“RSS”标签页——feed key（生成 / 重置 / 停用、最近使用）、订阅列表与编辑（类型、关键词、排除词、促销）、
// 订阅地址与 Torznab 地址（Prowlarr 配置说明）。
import { useCallback, useEffect, useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Button, Card, Empty, Field, Input, Table } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'rss.ui.tab': 'RSS',
  'rss.ui.key': '订阅密钥',
  'rss.ui.key_warning': '密钥可以以你的身份下载种子，不要分享给他人。重置后旧密钥立即失效，已配置的客户端需要更新。',
  'rss.ui.key_none': '还没有生成密钥',
  'rss.ui.key_current': '当前密钥：{prefix}…（只显示前 8 位）',
  'rss.ui.key_shown_once': '密钥只显示这一次，请立即保存：',
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
});
registerMessages('en', {
  'rss.ui.tab': 'RSS',
  'rss.ui.key': 'Feed key',
  'rss.ui.key_warning': 'The key downloads torrents on your account — do not share it. Resetting invalidates the old key immediately; update your clients afterwards.',
  'rss.ui.key_none': 'No key yet',
  'rss.ui.key_current': 'Current key: {prefix}… (first 8 characters)',
  'rss.ui.key_shown_once': 'The key is shown only once — save it now:',
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

export function SettingsTab({ api }: SlotProps) {
  const [key, setKey] = useState<KeyInfo>({ exists: false });
  const [secret, setSecret] = useState<string | null>(null);
  const [feeds, setFeeds] = useState<Feed[]>([]);
  const [torznab, setTorznab] = useState('');
  const [draft, setDraft] = useState<Draft>(EMPTY);
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
      setErr(errText(api, e));
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
    });
  const save = () =>
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
      setDraft(EMPTY);
    });
  const remove = (id: number) => act(async () => void (await api.fetch(`/api/p/rss/v1/me/feeds/${id}`, { method: 'DELETE' })));
  // 只有刚生成的密钥能拼出完整地址；之后显示占位符
  const url = (f: Feed) => f.url_template.replace('{key}', secret ?? '<key>');
  const set = (k: keyof Draft) => (e: { target: { value: string } }) => setDraft({ ...draft, [k]: e.target.value });
  return (
    <div className="pt-stack">
      <Card title={api.t('rss.ui.key')}>
        <p className="pt-muted">{api.t('rss.ui.key_warning')}</p>
        {key.exists ? (
          <p>
            {api.t('rss.ui.key_current', { prefix: key.prefix ?? '' })}
            <br />
            <span className="pt-muted">
              {key.last_used_at
                ? api.t('rss.ui.last_used', { at: new Date(key.last_used_at).toLocaleString(), ua: key.last_user_agent || '—' })
                : api.t('rss.ui.never')}
            </span>
          </p>
        ) : (
          <p>{api.t('rss.ui.key_none')}</p>
        )}
        {secret && (
          <p>
            {api.t('rss.ui.key_shown_once')} <code className="pt-mono" data-testid="rss-key">{secret}</code>
          </p>
        )}
        <div className="pt-row">
          <Button variant="primary" onClick={() => void resetKey()}>
            {api.t(key.exists ? 'rss.ui.reset' : 'rss.ui.generate')}
          </Button>
          {key.exists && (
            <Button variant="danger" onClick={() => void revokeKey()}>
              {api.t('rss.ui.revoke')}
            </Button>
          )}
        </div>
      </Card>
      <Card title={api.t('rss.ui.feeds')}>
        <Table
          rows={feeds}
          rowKey={(r) => String(r.id)}
          empty={<Empty>—</Empty>}
          columns={[
            { key: 'n', header: api.t('rss.ui.name'), render: (r) => r.name },
            { key: 'u', header: api.t('rss.ui.url'), render: (r) => <code className="pt-mono">{url(r)}</code> },
            {
              key: 'x',
              header: '',
              render: (r) => (
                <div className="pt-row">
                  <Button onClick={() => setDraft(toDraft(r))}>{api.t('rss.ui.edit')}</Button>
                  <Button variant="danger" onClick={() => void remove(r.id)}>
                    {api.t('rss.ui.delete')}
                  </Button>
                </div>
              ),
            },
          ]}
        />
        <div className="pt-row">
          <Field label={api.t('rss.ui.name')}>
            <Input value={draft.name} maxLength={60} onChange={set('name')} />
          </Field>
          <Field label={api.t('rss.ui.types')}>
            <Input value={draft.types} onChange={set('types')} />
          </Field>
          <Field label={api.t('rss.ui.q')}>
            <Input value={draft.q} maxLength={200} onChange={set('q')} />
          </Field>
        </div>
        <div className="pt-row">
          <Field label={api.t('rss.ui.exclude')}>
            <Input value={draft.exclude} onChange={set('exclude')} />
          </Field>
          <Field label={api.t('rss.ui.promotion')}>
            <Input value={draft.promotion} onChange={set('promotion')} />
          </Field>
        </div>
        <Button variant="primary" disabled={!draft.name.trim()} onClick={() => void save()}>
          {api.t(draft.id === null ? 'rss.ui.add' : 'rss.ui.save')}
        </Button>
        {err && <p className="pt-error">{err}</p>}
      </Card>
      <Card title={api.t('rss.ui.torznab')}>
        <p>
          <code className="pt-mono">{torznab}</code>
        </p>
        <p className="pt-muted">{api.t('rss.ui.torznab_hint')}</p>
      </Card>
    </div>
  );
}

export default {
  slots: {
    'user.settings.tabs': { component: SettingsTab, order: 50, title_term: 'rss.ui.tab' },
  },
} satisfies PluginFrontend;
