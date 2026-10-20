// 后台：外观——选择站点使用的前端主题插件（样式表 + 可选的替换页面），或使用框架自带的经典样式。
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { api, type FrontendManifest } from '@pt-studio/sdk';
import { useT } from '@pt-studio/terms';
import { Button, Note, PageHead, Panel, SkeletonRows, Status, useToast } from '@pt-studio/ui-kit';
import { ErrorText, useErrorText } from '../errors';
import { applyTheme } from '../theme';

interface Appearance {
  theme: string;
  themes: { id: string; name: string; summary: string }[];
}

const CLASSIC = 'classic';

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
      // 替换页面的主题需要重新加载才能完整生效；只换样式表的直接切换
      if (m.theme?.entry || document.documentElement.dataset.skinPages) window.location.reload();
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
