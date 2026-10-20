// 后台：管理操作日志（kernel.audit_log）——动作前缀常驻一行，插件 / 目标 / 操作者折叠在筛选里（P1）；点击行在抽屉里看详情。
import { useInfiniteQuery } from '@tanstack/react-query';
import { useState, type FormEvent } from 'react';
import { Link } from 'react-router';
import { api } from '@pt-studio/sdk';
import { useT } from '@pt-studio/terms';
import { ActiveFilters, Button, Field, FilterBar, Input, LoadMore, PageHead, Panel, Sheet, SkeletonRows, Status, Table } from '@pt-studio/ui-kit';
import { ErrorText } from '../errors';

interface Entry {
  id: number;
  occurred_at: number;
  actor: { kind?: string; user_id?: number | null; plugin_id?: string | null };
  plugin_id: string;
  action: string;
  target: string | null;
  outcome: string;
  request_id: string | null;
  details: Record<string, unknown>;
}

type Filter = { plugin: string; action: string; target: string; actor: string };
const EMPTY: Filter = { plugin: '', action: '', target: '', actor: '' };

export function AdminAudit() {
  const t = useT();
  const [draft, setDraft] = useState<Filter>(EMPTY);
  const [filter, setFilter] = useState<Filter>(EMPTY);
  const [adv, setAdv] = useState(false);
  const [open, setOpen] = useState<Entry | null>(null);
  const q = useInfiniteQuery({
    queryKey: ['admin-audit', filter],
    initialPageParam: null as number | null,
    queryFn: ({ pageParam }) => {
      const p = new URLSearchParams();
      for (const [k, v] of Object.entries(filter)) if (v.trim()) p.set(k, v.trim());
      if (pageParam) p.set('before', String(pageParam));
      return api<{ items: Entry[]; next_before: number | null }>(`/api/kernel/v1/admin/audit?${p.toString()}`);
    },
    getNextPageParam: (last) => last.next_before,
  });
  const rows = q.data?.pages.flatMap((p) => p.items) ?? [];
  const apply = (f: Filter) => {
    setDraft(f);
    setFilter(f);
  };
  const submit = (e: FormEvent) => {
    e.preventDefault();
    apply(draft);
  };
  const set = (k: keyof Filter) => (e: { target: { value: string } }) => setDraft({ ...draft, [k]: e.target.value });
  const labels: Record<keyof Filter, string> = { plugin: t('ui.audit.plugin'), action: t('ui.audit.action'), target: t('ui.audit.target'), actor: t('ui.audit.actor') };
  const active = (['plugin', 'target', 'actor'] as const).filter((k) => filter[k].trim());
  return (
    <div className="stack">
      <PageHead title={t('ui.audit.title')} />
      <Panel flush primary>
        <form onSubmit={submit}>
          <FilterBar
            open={adv}
            onOpenChange={setAdv}
            activeCount={active.length}
            onReset={() => apply({ ...EMPTY, action: draft.action })}
            title={t('ui.torrent.filters')}
            quick={<Input name="action" placeholder={t('ui.audit.action')} value={draft.action} onChange={set('action')} style={{ width: 240 }} />}
            right={
              <Button type="submit" size="sm" variant="primary">
                {t('ui.admin.search')}
              </Button>
            }
            active={<ActiveFilters items={active.map((k) => ({ key: k, label: labels[k], value: filter[k], onRemove: () => apply({ ...filter, [k]: '' }) }))} onClear={() => apply({ ...EMPTY, action: filter.action })} />}
            advanced={
              <div className="form-2">
                <Field label={labels.plugin}>
                  <Input name="plugin" value={draft.plugin} onChange={set('plugin')} />
                </Field>
                <Field label={labels.target}>
                  <Input name="target" placeholder="user:12" value={draft.target} onChange={set('target')} />
                </Field>
                <Field label={labels.actor}>
                  <Input name="actor" type="number" value={draft.actor} onChange={set('actor')} />
                </Field>
                <div className="fl">
                  <span className="fl-label">&nbsp;</span>
                  <Button type="submit" variant="primary" onClick={() => setAdv(false)}>
                    {t('ui.admin.search')}
                  </Button>
                </div>
              </div>
            }
          />
        </form>
        <ErrorText error={q.error} />
        {q.isLoading ? (
          <SkeletonRows n={8} />
        ) : (
          <Table
            rows={rows}
            rowKey={(r) => r.id}
            empty="—"
            minWidth={820}
            onRowClick={(r) => setOpen(r)}
            columns={[
              { key: 't', header: t('ui.settings.time'), render: (r) => <span className="nowrap small">{new Date(r.occurred_at).toLocaleString()}</span> },
              { key: 'a', header: t('ui.audit.actor'), render: (r) => (r.actor.user_id ? <Link to={`/admin/users/${r.actor.user_id}`}>#{r.actor.user_id}</Link> : <span className="muted">{r.actor.kind ?? '—'}</span>) },
              { key: 'p', header: t('ui.audit.plugin'), render: (r) => <span className="small">{r.plugin_id}</span> },
              { key: 'x', header: t('ui.audit.action'), render: (r) => <span className="row" style={{ gap: 6 }}><code className="mono">{r.action}</code>{r.outcome !== 'ok' && <Status tone="bad">{r.outcome}</Status>}</span> },
              { key: 'g', header: t('ui.audit.target'), render: (r) => <code className="mono">{r.target ?? ''}</code> },
            ]}
          />
        )}
        {q.hasNextPage && <LoadMore onClick={() => void q.fetchNextPage()} loading={q.isFetchingNextPage} label={t('ui.audit.more')} />}
      </Panel>
      <Sheet open={!!open} onClose={() => setOpen(null)} title={open ? open.action : ''} width={560}>
        {open && (
          <>
            <dl className="kv">
              <dt>{t('ui.settings.time')}</dt>
              <dd>{new Date(open.occurred_at).toLocaleString()}</dd>
              <dt>{t('ui.audit.plugin')}</dt>
              <dd>{open.plugin_id}</dd>
              <dt>{t('ui.audit.target')}</dt>
              <dd className="mono">{open.target ?? '—'}</dd>
              <dt>{t('ui.audit.actor')}</dt>
              <dd>{open.actor.user_id ? `#${open.actor.user_id}` : (open.actor.kind ?? '—')}</dd>
              {open.request_id && (
                <>
                  <dt>request</dt>
                  <dd className="mono">{open.request_id}</dd>
                </>
              )}
            </dl>
            <pre className="json">{JSON.stringify(open.details, null, 2)}</pre>
          </>
        )}
      </Sheet>
    </div>
  );
}
