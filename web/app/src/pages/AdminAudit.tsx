// 后台：管理操作日志（kernel.audit_log）——按插件、动作前缀、目标、操作者筛选，游标翻页，展开查看详情。
import { useInfiniteQuery } from '@tanstack/react-query';
import { useState, type FormEvent } from 'react';
import { Link } from 'react-router';
import { api } from '@pt-studio/sdk';
import { useT } from '@pt-studio/terms';
import { Badge, Button, Card, Input, Spinner, Table } from '@pt-studio/ui-kit';
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

export function AdminAudit() {
  const t = useT();
  const [draft, setDraft] = useState<Filter>({ plugin: '', action: '', target: '', actor: '' });
  const [filter, setFilter] = useState<Filter>(draft);
  const [open, setOpen] = useState<number | null>(null);
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
  const submit = (e: FormEvent) => {
    e.preventDefault();
    setFilter(draft);
  };
  const set = (k: keyof Filter) => (e: { target: { value: string } }) => setDraft({ ...draft, [k]: e.target.value });
  return (
    <div>
      <h1>{t('ui.audit.title')}</h1>
      <form className="searchbar" onSubmit={submit}>
        <Input name="plugin" placeholder={t('ui.audit.plugin')} value={draft.plugin} onChange={set('plugin')} />
        <Input name="action" placeholder={t('ui.audit.action')} value={draft.action} onChange={set('action')} />
        <Input name="target" placeholder={t('ui.audit.target')} value={draft.target} onChange={set('target')} />
        <Input name="actor" type="number" placeholder={t('ui.audit.actor')} value={draft.actor} onChange={set('actor')} />
        <Button type="submit">{t('ui.admin.search')}</Button>
      </form>
      <ErrorText error={q.error} />
      <Card>
        {q.isLoading ? (
          <Spinner />
        ) : (
          <Table
            rows={rows}
            rowKey={(r) => r.id}
            empty="—"
            columns={[
              { key: 't', header: t('ui.settings.time'), render: (r) => <span className="nowrap small">{new Date(r.occurred_at).toLocaleString()}</span> },
              {
                key: 'a',
                header: t('ui.audit.actor'),
                render: (r) => (r.actor.user_id ? <Link to={`/admin/users/${r.actor.user_id}`}>#{r.actor.user_id}</Link> : <span className="pt-muted">{r.actor.kind ?? '—'}</span>),
              },
              {
                key: 'x',
                header: t('ui.audit.action'),
                render: (r) => (
                  <>
                    <code className="pt-mono">{r.action}</code> {r.outcome !== 'ok' && <Badge tone="danger">{r.outcome}</Badge>}
                  </>
                ),
              },
              { key: 'g', header: t('ui.audit.target'), render: (r) => <code className="pt-mono small">{r.target ?? ''}</code> },
              {
                key: 'd',
                header: '',
                render: (r) =>
                  open === r.id ? (
                    <pre className="pt-mono small audit-details" onClick={() => setOpen(null)}>
                      {JSON.stringify(r.details, null, 2)}
                    </pre>
                  ) : (
                    <Button variant="ghost" onClick={() => setOpen(r.id)}>
                      {t('ui.audit.details')}
                    </Button>
                  ),
              },
            ]}
          />
        )}
        {q.hasNextPage && (
          <Button onClick={() => void q.fetchNextPage()} disabled={q.isFetchingNextPage}>
            {t('ui.audit.more')}
          </Button>
        )}
      </Card>
    </div>
  );
}
