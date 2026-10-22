// 后台：外观——站点信息（站名、字标）；选择站点使用的前端主题插件（样式表 + 可选的替换页面），或使用框架自带的经典样式。
import { useEffect, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { api, type FrontendManifest } from '@pt-studio/sdk';
import { useT } from '@pt-studio/terms';
import { Button, Field, Input, Note, PageHead, Panel, SkeletonRows, Status, useToast } from '@pt-studio/ui-kit';
import { ErrorText, useErrorText } from '../errors';
import { applyTheme } from '../theme';

interface Appearance {
  theme: string;
  themes: { id: string; name: string; summary: string }[];
}

const CLASSIC = 'classic';

interface SiteInfo {
  name: string;
  mark: [string, string];
}

/** 站名与字标（主字 + 副字）；保存后重新加载，让标题、字标与各处文案一起更新 */
function SitePanel() {
  const t = useT();
  const toast = useToast();
  const errText = useErrorText();
  const q = useQuery({ queryKey: ['admin-site'], queryFn: () => api<{ site: SiteInfo; default: SiteInfo }>('/api/kernel/v1/admin/site') });
  const [name, setName] = useState('');
  const [a, setA] = useState('');
  const [b, setB] = useState('');
  useEffect(() => {
    if (!q.data) return;
    setName(q.data.site.name);
    setA(q.data.site.mark[0]);
    setB(q.data.site.mark[1]);
  }, [q.data]);
  const save = useMutation({
    mutationFn: () => api('/api/kernel/v1/admin/site', { method: 'PUT', json: { name, mark: [a.trim() || name.trim(), a.trim() ? b : ''] } }),
    onSuccess: () => {
      toast(t('ui.site.saved'), 'success');
      window.location.reload();
    },
    onError: (e) => toast(errText(e), 'error'),
  });
  if (q.error) return <ErrorText error={q.error} />;
  if (!q.data) return <SkeletonRows n={2} />;
  const main = a.trim() || name.trim();
  const dirty = name !== q.data.site.name || a !== q.data.site.mark[0] || b !== q.data.site.mark[1];
  return (
    <Panel title={t('ui.site.title')} sub={t('ui.site.hint', { name: q.data.default.name })}>
      <form
        className="stack"
        onSubmit={(e) => {
          e.preventDefault();
          save.mutate();
        }}
      >
        <Field label={t('ui.site.name')} hint={t('ui.site.name_hint')}>
          <Input value={name} maxLength={40} required onChange={(e) => setName(e.target.value)} />
        </Field>
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(200px, 1fr))', gap: 12, alignItems: 'start' }}>
          <Field label={t('ui.site.mark_main')}>
            <Input value={a} maxLength={40} placeholder={name} onChange={(e) => setA(e.target.value)} />
          </Field>
          <Field label={t('ui.site.mark_sub')} hint={t('ui.site.mark_hint')}>
            <Input value={b} maxLength={40} onChange={(e) => setB(e.target.value)} />
          </Field>
        </div>
        <div className="row-sb">
          <span className="mark" aria-label={t('ui.site.preview')}>
            <b>{main}</b>
            {b.trim() && <i>{b.trim()}</i>}
          </span>
          <Button variant="primary" type="submit" disabled={!dirty || !name.trim() || save.isPending}>
            {t('ui.site.save')}
          </Button>
        </div>
      </form>
    </Panel>
  );
}

export function AdminAppearance() {
  const t = useT();
  const toast = useToast();
  const errText = useErrorText();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ['admin-appearance'], queryFn: () => api<Appearance>('/api/kernel/v1/admin/appearance') });
  const save = useMutation({
    mutationFn: async (theme: string) => {
      await api('/api/kernel/v1/admin/appearance', { method: 'PUT', json: { theme } });
      const m = await api<FrontendManifest>('/api/kernel/v1/frontend/manifest');
      // 替换页面或覆盖术语的主题需要重新加载才能完整生效；只换样式表的直接切换
      const root = document.documentElement.dataset;
      if (m.theme?.entry || m.theme?.terms || root.skinPages || root.skinTerms) window.location.reload();
      else await applyTheme(m.theme ?? null);
    },
    onSuccess: () => {
      toast(t('ui.appearance.saved'), 'success');
      void qc.invalidateQueries({ queryKey: ['admin-appearance'] });
    },
    onError: (e) => toast(errText(e), 'error'),
  });
  const d = q.data;
  const options = d ? [{ id: CLASSIC, name: t('ui.appearance.classic'), summary: t('ui.appearance.classic_hint') }, ...d.themes] : [];
  return (
    <div className="stack">
      <PageHead title={t('ui.appearance.title')} lede={t('ui.appearance.lede')} />
      <SitePanel />
      {q.error ? (
        <ErrorText error={q.error} />
      ) : !d ? (
        <SkeletonRows n={3} />
      ) : (
        <Panel title={t('ui.appearance.theme')} sub={t('ui.appearance.theme_hint')}>
          <div className="skins">
            {options.map((o) => {
              const on = d.theme === o.id;
              return (
                <div key={o.id} className="skin" data-on={on || undefined}>
                  <div className="skin-pv" data-pv={o.id} aria-hidden="true">
                    <i />
                    <i />
                    <i />
                  </div>
                  <div className="skin-b">
                    <div className="row-sb">
                      <b>{o.name}</b>
                      {on && <Status tone="ok">{t('ui.appearance.current')}</Status>}
                    </div>
                    <p className="small muted">{o.summary}</p>
                    {o.id !== CLASSIC && <span className="mono tiny muted">{o.id}</span>}
                  </div>
                  {!on && (
                    <Button size="sm" disabled={save.isPending} onClick={() => save.mutate(o.id)}>
                      {t('ui.appearance.use')}
                    </Button>
                  )}
                </div>
              );
            })}
          </div>
          {d.themes.length === 0 && <Note tone="info">{t('ui.appearance.none')}</Note>}
        </Panel>
      )}
    </div>
  );
}
