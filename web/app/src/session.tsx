// 会话：当前用户、角色、权限（identity `/users/me`、`/users/me/permissions`）。
import { useQueryClient } from '@tanstack/react-query';
import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from 'react';
import { api, ApiError, type Me } from '@pt-studio/sdk';

export interface Session {
  me: Me | null;
  roles: string[];
  permissions: string[];
  /** 邮箱是否已验证（未验证时外壳显示提示条） */
  emailVerified: boolean;
  /** 角色要求两步验证、本会话尚未通过（员工权限暂不生效） */
  twoFactorNeeded: boolean;
  loading: boolean;
  refresh(): Promise<void>;
  logout(): Promise<void>;
  can(perm: string): boolean;
  isStaff: boolean;
}

const Ctx = createContext<Session | null>(null);

export function SessionProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<{ me: Me | null; roles: string[]; permissions: string[]; emailVerified: boolean; twoFactorNeeded: boolean; loading: boolean }>({
    me: null,
    roles: [],
    permissions: [],
    emailVerified: true,
    twoFactorNeeded: false,
    loading: true,
  });
  const qc = useQueryClient();
  // 查询缓存里有大量“我的”数据（键里不含用户 ID）：换账号时整体清空，避免看到上一个账号的数据
  const lastUser = useRef<number | null | undefined>(undefined);
  const switchUser = useCallback(
    (id: number | null) => {
      if (lastUser.current !== undefined && lastUser.current !== id) qc.clear();
      lastUser.current = id;
    },
    [qc],
  );
  const refresh = useCallback(async () => {
    try {
      const r = await api<{ user: Me; roles: string[]; email_verified?: boolean }>('/api/p/identity/v1/users/me');
      const p = await api<{ permissions: string[]; two_factor_needed?: boolean }>('/api/p/identity/v1/users/me/permissions');
      switchUser(r.user.id);
      setState({
        me: r.user,
        roles: r.roles,
        permissions: p.permissions,
        emailVerified: r.email_verified !== false,
        twoFactorNeeded: p.two_factor_needed === true,
        loading: false,
      });
    } catch (e) {
      if (!(e instanceof ApiError) || e.status !== 401) console.warn(e);
      switchUser(null);
      setState({ me: null, roles: [], permissions: [], emailVerified: true, twoFactorNeeded: false, loading: false });
    }
  }, [switchUser]);
  const logout = useCallback(async () => {
    await api('/api/p/identity/v1/sessions/current', { method: 'DELETE' }).catch(() => undefined);
    switchUser(null);
    qc.clear();
    setState({ me: null, roles: [], permissions: [], emailVerified: true, twoFactorNeeded: false, loading: false });
  }, [switchUser, qc]);
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
