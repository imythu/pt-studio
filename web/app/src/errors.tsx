import { ApiError } from '@pt-studio/sdk';
import { useT } from '@pt-studio/terms';

/** 错误码 → 本地化文案（未知错误码显示服务端 message） */
export function useErrorText(): (e: unknown) => string {
  const t = useT();
  return (e) => {
    if (e instanceof ApiError) {
      const s = t(e.code);
      return s === e.code ? e.message || e.code : s;
    }
    return e instanceof Error ? e.message : String(e);
  };
}

export function ErrorText({ error }: { error: unknown }) {
  const text = useErrorText();
  if (!error) return null;
  return (
    <p className="pt-error" role="alert">
      {text(error)}
    </p>
  );
}
