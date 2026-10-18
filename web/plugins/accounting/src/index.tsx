// accounting 前端：后台用户详情页“流量”标签 —— 当前上传 / 下载 / 分享率与手动调整（GiB，可为负，需填写原因）。
import { useEffect, useState, type FormEvent } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { formatBytes, registerMessages } from '@pt-studio/terms';
import { Button, Card, Field, Input, Stat } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'accounting.ui.tab': '流量',
  'accounting.ui.uploaded': '上传量',
  'accounting.ui.downloaded': '下载量',
  'accounting.ui.ratio': '分享率',
  'accounting.ui.d_up': '上传量调整（GiB，可为负）',
  'accounting.ui.d_down': '下载量调整（GiB，可为负）',
  'accounting.ui.note': '原因（必填，写入审计）',
  'accounting.ui.apply': '调整流量',
  'accounting.ui.done': '已调整',
});
registerMessages('en', {
  'accounting.ui.tab': 'Traffic',
  'accounting.ui.uploaded': 'Uploaded',
  'accounting.ui.downloaded': 'Downloaded',
  'accounting.ui.ratio': 'Ratio',
  'accounting.ui.d_up': 'Upload change (GiB, may be negative)',
  'accounting.ui.d_down': 'Download change (GiB, may be negative)',
  'accounting.ui.note': 'Reason (required, audited)',
  'accounting.ui.apply': 'Adjust traffic',
  'accounting.ui.done': 'Adjusted',
});

interface Stats {
  uploaded: number;
  downloaded: number;
}

const errText = (api: PluginApi, e: unknown) =>
  e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e);
const GIB = 1024 ** 3;

function UserTab({ ctx, api }: SlotProps<{ profile: { id: number } }>) {
  const uid = ctx.profile.id;
  const [s, setS] = useState<Stats | null>(null);
  const [f, setF] = useState({ up: '', down: '', note: '' });
  const load = () => void api.json<Stats>(`/api/p/accounting/v1/users/${uid}/stats`).then(setS);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(load, [uid]);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    try {
      await api.json(`/api/p/accounting/v1/admin/users/${uid}/adjustments`, {
        json: { d_up: Math.round(Number(f.up || 0) * GIB), d_down: Math.round(Number(f.down || 0) * GIB), note: f.note },
        idempotent: true,
      });
      api.toast(api.t('accounting.ui.done'), 'success');
      setF({ up: '', down: '', note: '' });
      load();
    } catch (err) {
      api.toast(errText(api, err), 'error');
    }
  };
  if (!s) return null;
  const ratio = s.downloaded > 0 ? (s.uploaded / s.downloaded).toFixed(3) : '∞';
  return (
    <Card>
      <div className="pt-row stats-row">
        <Stat label={api.t('accounting.ui.uploaded')} value={formatBytes(s.uploaded)} />
        <Stat label={api.t('accounting.ui.downloaded')} value={formatBytes(s.downloaded)} />
        <Stat label={api.t('accounting.ui.ratio')} value={ratio} />
      </div>
      <form onSubmit={(e) => void submit(e)} className="upload-grid">
        <Field label={api.t('accounting.ui.d_up')}>
          <Input name="traffic_up" type="number" step="any" value={f.up} onChange={(e) => setF({ ...f, up: e.target.value })} />
        </Field>
        <Field label={api.t('accounting.ui.d_down')}>
          <Input name="traffic_down" type="number" step="any" value={f.down} onChange={(e) => setF({ ...f, down: e.target.value })} />
        </Field>
        <div className="span2">
          <Field label={api.t('accounting.ui.note')}>
            <Input name="traffic_note" required maxLength={500} value={f.note} onChange={(e) => setF({ ...f, note: e.target.value })} />
          </Field>
          <Button type="submit" variant="danger" disabled={!f.note.trim() || (!Number(f.up) && !Number(f.down))}>
            {api.t('accounting.ui.apply')}
          </Button>
        </div>
      </form>
    </Card>
  );
}

export default {
  slots: {
    'admin.user.tabs': { component: UserTab, order: 10, title_term: 'accounting.ui.tab' },
  },
  routes: [],
} satisfies PluginFrontend;
