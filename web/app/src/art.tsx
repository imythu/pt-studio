// 品牌标志与插画（内联 SVG，颜色取主题令牌，亮 / 暗主题自动适配）。

export function Logo({ size = 32 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 64 64" aria-hidden="true">
      <defs>
        <linearGradient id="pt-logo-g" x1="6" y1="4" x2="58" y2="60" gradientUnits="userSpaceOnUse">
          <stop offset="0" stopColor="#2f6df6" />
          <stop offset="1" stopColor="#5b8bff" />
        </linearGradient>
      </defs>
      <rect width="64" height="64" rx="16" fill="url(#pt-logo-g)" />
      <g fill="none" stroke="#fff" strokeWidth="5" strokeLinecap="round" strokeLinejoin="round">
        <path d="M23 46V19M14 28l9-9 9 9" />
        <path d="M41 18v27M32 36l9 9 9-9" opacity=".92" />
      </g>
    </svg>
  );
}

/** 首页 / 登录页主插画：分享网络、资源卡片与传输进度 */
export function HeroArt({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="-10 -20 540 480" fill="none" overflow="visible" role="img" aria-hidden="true">
      <defs>
        <linearGradient id="ha-g1" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#2f6df6" />
          <stop offset="1" stopColor="#5b8bff" />
        </linearGradient>
        <linearGradient id="ha-g2" x1="0" y1="1" x2="1" y2="0">
          <stop offset="0" stopColor="#7aa7ff" />
          <stop offset="1" stopColor="#2f6df6" />
        </linearGradient>
        <radialGradient id="ha-glow" cx=".5" cy=".5" r=".5">
          <stop offset="0" stopColor="#2f6df6" stopOpacity=".38" />
          <stop offset="1" stopColor="#2f6df6" stopOpacity="0" />
        </radialGradient>
        <filter id="ha-shadow" filterUnits="userSpaceOnUse" x="-60" y="-60" width="700" height="640">
          <feDropShadow dx="0" dy="14" stdDeviation="14" floodColor="#2f6df6" floodOpacity=".22" />
        </filter>
      </defs>

      <circle cx="270" cy="220" r="210" fill="url(#ha-glow)" />
      <circle cx="270" cy="220" r="168" stroke="var(--pt-color-border)" strokeWidth="1.5" strokeDasharray="3 9" />
      <circle cx="270" cy="220" r="116" stroke="var(--pt-color-border)" strokeWidth="1.5" strokeDasharray="3 9" />

      {/* 连线 */}
      <g stroke="url(#ha-g1)" strokeWidth="2" strokeLinecap="round" strokeDasharray="5 7" opacity=".7">
        <path d="M270 220 112 110" />
        <path d="M270 220 428 96" />
        <path d="M270 220 450 292" />
        <path d="M270 220 118 330" />
        <path d="M270 220 270 52" />
      </g>

      {/* 节点 */}
      <g>
        <circle cx="112" cy="110" r="22" fill="var(--pt-color-surface)" stroke="#2f6df6" strokeWidth="2.5" />
        <path d="M112 102v16m-6-10 6-6 6 6" stroke="#2f6df6" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round" />
        <circle cx="428" cy="96" r="18" fill="var(--pt-color-surface)" stroke="#5b8bff" strokeWidth="2.5" />
        <path d="M428 88v16m-6-6 6 6 6-6" stroke="#5b8bff" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round" />
        <circle cx="450" cy="292" r="24" fill="var(--pt-color-surface)" stroke="#7aa7ff" strokeWidth="2.5" />
        <path d="M450 284v16m-6-10 6-6 6 6" stroke="#7aa7ff" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round" />
        <circle cx="118" cy="330" r="19" fill="var(--pt-color-surface)" stroke="#2f6df6" strokeWidth="2.5" />
        <path d="M118 322v16m-6-6 6 6 6-6" stroke="#2f6df6" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round" />
        <circle cx="270" cy="52" r="15" fill="url(#ha-g1)" />
        <circle cx="270" cy="52" r="5" fill="#fff" />
      </g>

      {/* 主卡片 */}
      <g filter="url(#ha-shadow)">
        <rect x="170" y="140" width="200" height="160" rx="20" fill="var(--pt-color-surface)" stroke="var(--pt-color-border)" />
      </g>
      <rect x="188" y="158" width="44" height="44" rx="12" fill="url(#ha-g1)" />
      <path d="M204 188v-14m-6 6 6-6 6 6" stroke="#fff" strokeWidth="3.5" strokeLinecap="round" strokeLinejoin="round" />
      <rect x="244" y="162" width="104" height="9" rx="4.5" fill="var(--pt-color-fg)" opacity=".85" />
      <rect x="244" y="181" width="68" height="7" rx="3.5" fill="var(--pt-color-muted)" opacity=".5" />
      <rect x="188" y="222" width="164" height="10" rx="5" fill="var(--pt-color-surface-2)" />
      <rect x="188" y="222" width="112" height="10" rx="5" fill="url(#ha-g1)" />
      <rect x="188" y="248" width="52" height="22" rx="11" fill="var(--pt-color-primary-soft)" />
      <rect x="248" y="248" width="40" height="22" rx="11" fill="var(--pt-color-success-soft)" />
      <rect x="296" y="248" width="56" height="22" rx="11" fill="var(--pt-color-surface-2)" />

      {/* 浮动小卡片 */}
      <g filter="url(#ha-shadow)">
        <rect x="324" y="318" width="132" height="64" rx="16" fill="var(--pt-color-surface)" stroke="var(--pt-color-border)" />
      </g>
      <circle cx="352" cy="350" r="14" fill="url(#ha-g2)" />
      <path d="m346 350 5 5 9-10" stroke="#fff" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round" />
      <rect x="376" y="338" width="64" height="8" rx="4" fill="var(--pt-color-fg)" opacity=".8" />
      <rect x="376" y="354" width="42" height="7" rx="3.5" fill="var(--pt-color-muted)" opacity=".5" />

      <g filter="url(#ha-shadow)">
        <rect x="64" y="204" width="120" height="58" rx="16" fill="var(--pt-color-surface)" stroke="var(--pt-color-border)" />
      </g>
      <path d="M92 240v-16m-7 7 7-7 7 7" stroke="#5b8bff" strokeWidth="3.5" strokeLinecap="round" strokeLinejoin="round" />
      <rect x="112" y="223" width="56" height="8" rx="4" fill="var(--pt-color-fg)" opacity=".8" />
      <rect x="112" y="238" width="36" height="7" rx="3.5" fill="var(--pt-color-muted)" opacity=".5" />

      {/* 装饰 */}
      <path d="M388 176l3 8 8 3-8 3-3 8-3-8-8-3 8-3z" fill="#5b8bff" opacity=".6" />
      <path d="M96 52l2 5 5 2-5 2-2 5-2-5-5-2 5-2z" fill="#7aa7ff" opacity=".7" />
      <circle cx="486" cy="188" r="4" fill="#2f6df6" opacity=".5" />
      <circle cx="58" cy="400" r="5" fill="#5b8bff" opacity=".4" />
      <circle cx="210" cy="402" r="3" fill="#7aa7ff" opacity=".6" />
    </svg>
  );
}

/** 页面 404 / 无权限插画 */
export function LostArt({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 240 160" fill="none" aria-hidden="true">
      <ellipse cx="120" cy="146" rx="78" ry="8" fill="currentColor" opacity=".07" />
      <rect x="42" y="30" width="156" height="100" rx="16" fill="var(--pt-color-surface)" stroke="var(--pt-color-border)" strokeWidth="2" />
      <rect x="42" y="30" width="156" height="26" rx="16" fill="var(--pt-color-surface-2)" />
      <circle cx="60" cy="43" r="4" fill="#ff6b6b" />
      <circle cx="74" cy="43" r="4" fill="#fbbf24" />
      <circle cx="88" cy="43" r="4" fill="#34d399" />
      <text x="120" y="108" textAnchor="middle" fontSize="40" fontWeight="800" fill="url(#lost-g)">404</text>
      <defs>
        <linearGradient id="lost-g" x1="0" y1="0" x2="1" y2="0">
          <stop offset="0" stopColor="#2f6df6" />
          <stop offset="1" stopColor="#5b8bff" />
        </linearGradient>
      </defs>
      <circle cx="196" cy="34" r="10" fill="var(--pt-color-primary-soft)" stroke="var(--pt-color-primary)" strokeWidth="2" />
      <path d="m203 41 8 8" stroke="var(--pt-color-primary)" strokeWidth="3" strokeLinecap="round" />
    </svg>
  );
}

const ICONS = {
  search: 'M11 4a7 7 0 1 0 4.2 12.6l4.1 4.1 1.4-1.4-4.1-4.1A7 7 0 0 0 11 4Zm0 2a5 5 0 1 1 0 10 5 5 0 0 1 0-10Z',
  sun: 'M12 7a5 5 0 1 0 0 10 5 5 0 0 0 0-10Zm0-5h0v3M12 19v3M4.2 4.2l2.1 2.1M17.7 17.7l2.1 2.1M2 12h3M19 12h3M4.2 19.8l2.1-2.1M17.7 6.3l2.1-2.1',
  moon: 'M20 14.5A8 8 0 0 1 9.5 4 8 8 0 1 0 20 14.5Z',
  upload: 'M12 16V4m-5 5 5-5 5 5M4 20h16',
  menu: 'M4 7h16M4 12h16M4 17h16',
  close: 'M6 6l12 12M18 6 6 18',
  user: 'M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8Zm-8 9a8 8 0 0 1 16 0',
  settings: 'M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6Zm7.4-2.5.1-1 2-1.5-2-3.5-2.3.9a7 7 0 0 0-1.7-1L15 3.5h-4l-.5 2.4a7 7 0 0 0-1.7 1l-2.3-.9-2 3.5 2 1.5a7 7 0 0 0 0 2l-2 1.5 2 3.5 2.3-.9c.5.4 1.1.7 1.7 1l.5 2.4h4l.5-2.4c.6-.3 1.2-.6 1.7-1l2.3.9 2-3.5-2-1.5Z',
  logout: 'M15 4h4a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1h-4M10 16l-4-4 4-4M6 12h10',
  shield: 'M12 3 4 6v6c0 4.5 3.4 8.2 8 9 4.6-.8 8-4.5 8-9V6l-8-3Z',
  film: 'M4 5h16v14H4zM8 5v14M16 5v14M4 9h4M4 15h4M16 9h4M16 15h4',
  zap: 'M13 2 4 14h7l-1 8 9-12h-7l1-8Z',
  users: 'M16 11a3 3 0 1 0 0-6 3 3 0 0 0 0 6ZM8 12a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7Zm0 2c-3 0-6 1.5-6 4.5V20h12v-1.5C14 15.5 11 14 8 14Zm8 1c2.5 0 6 1 6 3.5V20h-6',
  arrow: 'M5 12h14m-5-5 5 5-5 5',
  home: 'M3 11 12 3l9 8M5 10v10h5v-6h4v6h5V10',
  download: 'M12 4v12m-5-5 5 5 5-5M4 20h16',
  bell: 'M6 9a6 6 0 1 1 12 0c0 6 2 7.5 2 7.5H4S6 15 6 9Zm4 10a2 2 0 0 0 4 0',
  mail: 'M4 6h16v12H4zM4 7l8 6 8-6',
  chevron: 'm6 9 6 6 6-6',
} as const;

export type IconName = keyof typeof ICONS;

export function Icon({ name, size = 18 }: { name: IconName; size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d={ICONS[name]} />
    </svg>
  );
}
