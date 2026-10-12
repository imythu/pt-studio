// 会话：当前用户、角色、权限（identity `/users/me`、`/users/me/permissions`）。
import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from 'react';
import { api, ApiError, type Me } from '@pt-studio/sdk';

export interface Session {
  me: Me | null;
  roles: string[];
  permissions: string[];
  loading: boolean;
  refresh(): Promise<void>;
  logout(): Promise<void>;
  can(perm: string): boolean;
  isStaff: boolean;
}

const Ctx = createContext<Session | null>(null);

export function SessionProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<{ me: Me | null; roles: string[]; permissions: string[]; loading: boolean }>({
    me: null,
    roles: [],
    permissions: [],
    loading: true,
  });
  const refresh = useCallback(async () => {
    try {
      const r = await api<{ user: Me; roles: string[] }>('/api/p/identity/v1/users/me');
      const p = await api<{ permissions: string[] }>('/api/p/identity/v1/users/me/permissions');
      setState({ me: r.user, roles: r.roles, permissions: p.permissions, loading: false });
    } catch (e) {
      if (!(e instanceof ApiError) || e.status !== 401) console.warn(e);
      setState({ me: null, roles: [], permissions: [], loading: false });
    }
  }, []);
  const logout = useCallback(async () => {
    await api('/api/p/identity/v1/sessions/current', { method: 'DELETE' }).catch(() => undefined);
    setState({ me: null, roles: [], permissions: [], loading: false });
  }, []);
  useEffect(() => {
    void refresh();
  }, [refresh]);
  const value: Session = {
    ...state,
    refresh,
    logout,
    can: (perm) => state.permissions.includes(perm),
    isStaff: state.roles.some((r) => r === 'moderator' || r === 'admin' || r === 'sysop'),
  };
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useSession(): Session {
  const s = useContext(Ctx);
  if (!s) throw new Error('SessionProvider is missing');
  return s;
}
