import { useLocation, useParams } from 'react-router';
import { PluginPage } from '@pt-studio/slots';
import { Spinner } from '@pt-studio/ui-kit';

/** `/p/:plugin/*`：交给插件前端声明的路由组件 */
export function PluginRoute() {
  const { plugin = '' } = useParams();
  const loc = useLocation();
  return <PluginPage pluginId={plugin} path={loc.pathname} fallback={<Spinner />} />;
}
