// 搜索联想（search-meilisearch `/suggest`）：输入两个字符以上时按标题前缀给出种子候选，选中直接打开种子页。
// 插件未安装或请求失败时不显示。
import { useEffect, useId, useState, type InputHTMLAttributes, type KeyboardEvent, type Ref } from 'react';
import { useNavigate } from 'react-router';
import { useQuery } from '@tanstack/react-query';
import { api } from '@pt-studio/sdk';
import { useInstalled } from './manifest';
import { useSession } from './session';

interface Hit {
  id: number | string;
  text: string;
}

function useDebounced(v: string, ms: number): string {
  const [d, setD] = useState(v);
  useEffect(() => {
    const h = setTimeout(() => setD(v), ms);
    return () => clearTimeout(h);
  }, [v, ms]);
  return d;
}

/** 带联想下拉的搜索输入框；`value` / `onValue` 受控，回车（未选中候选时）交给外层表单提交 */
export function SuggestInput({
  value,
  onValue,
  onPick,
  ref,
  className,
  ...rest
}: Omit<InputHTMLAttributes<HTMLInputElement>, 'value' | 'onChange'> & { value: string; onValue(v: string): void; onPick?(): void; ref?: Ref<HTMLInputElement> }) {
  const nav = useNavigate();
  const listId = useId();
  const installed = useInstalled('search-meilisearch');
  const enabled = useSession().me !== null && installed;
  const q = useDebounced(value.trim(), 200);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(-1);
  const r = useQuery({
    queryKey: ['suggest', q],
    queryFn: () => api<{ items: Hit[] }>(`/api/p/search-meilisearch/v1/suggest?index=torrents&limit=8&prefix=${encodeURIComponent(q)}`),
    enabled: enabled && q.length >= 2,
    staleTime: 30_000,
    retry: false,
  });
  const hits = open && q.length >= 2 ? (r.data?.items ?? []) : [];
  useEffect(() => setActive(-1), [q]);
  const pick = (h: Hit) => {
    setOpen(false);
    onValue('');
    nav(`/torrents/${h.id}`);
    onPick?.();
  };
  const key = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Escape') {
      setOpen(false);
      (e.target as HTMLInputElement).blur();
      return;
    }
    if (hits.length === 0) return;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const n = hits.length;
      setActive((a) => (e.key === 'ArrowDown' ? (a + 1) % n : (a - 1 + n) % n));
    } else if (e.key === 'Enter' && active >= 0) {
      e.preventDefault();
      pick(hits[active] as Hit);
    }
  };
  return (
    <span className="suggest">
      <input
        {...rest}
        ref={ref}
        className={className}
        value={value}
        role="combobox"
        aria-autocomplete="list"
        aria-expanded={hits.length > 0}
        aria-controls={listId}
        aria-activedescendant={active >= 0 ? `${listId}-${active}` : undefined}
        autoComplete="off"
        onChange={(e) => {
          onValue(e.target.value);
          setOpen(true);
        }}
        onFocus={(e) => {
          setOpen(true);
          rest.onFocus?.(e);
        }}
        onBlur={(e) => {
          setOpen(false);
          rest.onBlur?.(e);
        }}
        onKeyDown={key}
      />
      {hits.length > 0 && (
        <ul id={listId} role="listbox" className="suggest-list">
          {hits.map((h, i) => (
            <li
              key={h.id}
              id={`${listId}-${i}`}
              role="option"
              aria-selected={i === active}
              // mousedown 先于 input 失焦，保证点击生效
              onMouseDown={(e) => {
                e.preventDefault();
                pick(h);
              }}
            >
              {h.text}
            </li>
          ))}
        </ul>
      )}
    </span>
  );
}
