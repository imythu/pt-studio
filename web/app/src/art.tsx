// 品牌标志与插画（内联 SVG，颜色取主题令牌，亮 / 暗主题自动适配）。

/** 黑猫脸：站标、欢迎横幅等处共用（颜色自带，不随主题变化） */
export function CatFace({ size = 32, bg = true }: { size?: number; bg?: boolean }) {
  return (
    <svg width={size} height={size} viewBox="0 0 64 64" aria-hidden="true">
      <defs>
        <linearGradient id="cat-bg" x1="6" y1="4" x2="58" y2="60" gradientUnits="userSpaceOnUse">
          <stop offset="0" stopColor="#8fdcc3" />
          <stop offset="1" stopColor="#7aa7f5" />
        </linearGradient>
      </defs>
      {bg && <rect width="64" height="64" rx="16" fill="url(#cat-bg)" />}
      <path d="M13 31 15 9l16 12Z M51 31 49 9 33 21Z" fill="#1d2130" strokeLinejoin="round" />
      <path d="M17 26 18 15l8 6Z M47 26 46 15l-8 6Z" fill="#f7a8b8" opacity=".85" />
      <ellipse cx="32" cy="37" rx="21" ry="18" fill="#1d2130" />
      <ellipse cx="23.5" cy="36" rx="6.2" ry="7.2" fill="#ffd66e" />
      <ellipse cx="40.5" cy="36" rx="6.2" ry="7.2" fill="#ffd66e" />
      <ellipse cx="23.8" cy="36.4" rx="2.6" ry="5.4" fill="#1d2130" />
      <ellipse cx="40.2" cy="36.4" rx="2.6" ry="5.4" fill="#1d2130" />
      <circle cx="21.8" cy="33" r="1.9" fill="#fff" />
      <circle cx="38.2" cy="33" r="1.9" fill="#fff" />
      <path d="M30 43h4l-2 2.4Z" fill="#f7a8b8" />
      <path d="M32 45.4v1.6m-3.4.6c1.2 1.4 2.6 1.4 3.4 0 .8 1.4 2.2 1.4 3.4 0" fill="none" stroke="#f7a8b8" strokeWidth="1.1" strokeLinecap="round" />
      <ellipse cx="15.5" cy="43" rx="3.2" ry="2" fill="#f7a8b8" opacity=".35" />
      <ellipse cx="48.5" cy="43" rx="3.2" ry="2" fill="#f7a8b8" opacity=".35" />
    </svg>
  );
}

export function Logo({ size = 32 }: { size?: number }) {
  return <CatFace size={size} />;
}

/** 首页 / 登录页主插画：黄昏花海里的魔法使与黑猫（原创，自带夜空背景，亮暗主题通用） */
export function HeroArt({ className }: { className?: string }) {
  const flowers: [number, number, string][] = [
    [40, 392, '#fff'], [78, 410, '#ffd0dc'], [118, 398, '#fff6b5'], [262, 404, '#fff'], [296, 418, '#ffd0dc'], [214, 420, '#fff6b5'],
    [418, 400, '#fff'], [452, 416, '#ffd0dc'], [488, 396, '#fff6b5'], [372, 422, '#fff'], [150, 424, '#ffd0dc'], [20, 424, '#fff6b5'],
  ];
  return (
    <svg className={className} viewBox="0 0 520 440" fill="none" role="img" aria-hidden="true">
      <defs>
        <linearGradient id="ha-sky" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#222d5a" />
          <stop offset=".55" stopColor="#5a6dc2" />
          <stop offset="1" stopColor="#f7d3bd" />
        </linearGradient>
        <radialGradient id="ha-moon" cx=".5" cy=".5" r=".5">
          <stop offset="0" stopColor="#fff4cf" stopOpacity=".55" />
          <stop offset="1" stopColor="#fff4cf" stopOpacity="0" />
        </radialGradient>
        <radialGradient id="ha-orb" cx=".5" cy=".5" r=".5">
          <stop offset="0" stopColor="#d8f3ff" />
          <stop offset="1" stopColor="#d8f3ff" stopOpacity="0" />
        </radialGradient>
        <clipPath id="ha-clip">
          <rect width="520" height="440" rx="36" />
        </clipPath>
      </defs>
      <g clipPath="url(#ha-clip)">
        <rect width="520" height="440" fill="url(#ha-sky)" />
        <circle cx="392" cy="112" r="92" fill="url(#ha-moon)" />
        <circle cx="392" cy="112" r="42" fill="#fff4cf" />
        <circle cx="380" cy="102" r="7" fill="#f3e3b0" opacity=".7" />
        <circle cx="404" cy="126" r="5" fill="#f3e3b0" opacity=".7" />
        {([[60, 60], [130, 120], [220, 48], [300, 96], [468, 58], [486, 168], [40, 190], [250, 160]] as [number, number][]).map(([x, y], i) => (
          <path key={i} d={`M${x} ${y - 6}l1.8 4.2 4.2 1.8-4.2 1.8L${x} ${y + 6}l-1.8-4.2-4.2-1.8 4.2-1.8Z`} fill="#fff" opacity={0.5 + (i % 3) * 0.2} />
        ))}
        <path d="M0 292C80 250 170 262 250 280S430 268 520 288V440H0Z" fill="#6f7fcf" opacity=".85" />
        <path d="M0 338C110 304 210 326 300 332S452 318 520 338V440H0Z" fill="#5fb89d" />
        <path d="M0 372C120 352 240 368 340 372S470 364 520 374V440H0Z" fill="#4aa58a" />
        {flowers.map(([x, y, c], i) => (
          <g key={i}>
            {[0, 72, 144, 216, 288].map((r) => (
              <ellipse key={r} cx={x} cy={y - 4.5} rx="2.6" ry="4" fill={c} transform={`rotate(${r} ${x} ${y})`} />
            ))}
            <circle cx={x} cy={y} r="2" fill="#f0b94a" />
          </g>
        ))}

        {/* 魔法使 */}
        <g>
          <path d="M240 392 250 214" stroke="#8a5a3c" strokeWidth="6" strokeLinecap="round" />
          <circle cx="248" cy="196" r="34" fill="url(#ha-orb)" />
          <circle cx="248" cy="196" r="12" fill="#e6f7ff" stroke="#f0b94a" strokeWidth="3" />
          <path d="M230 196c2-20 34-20 36 0" stroke="#f0b94a" strokeWidth="4" strokeLinecap="round" />
          <path d="M126 150c-22 44-14 120-2 168" stroke="#f1f3fb" strokeWidth="16" strokeLinecap="round" />
          <path d="M214 150c22 44 14 120 2 168" stroke="#f1f3fb" strokeWidth="16" strokeLinecap="round" />
          <path d="M150 262Q170 246 190 262L228 388Q170 404 112 388Z" fill="#f8f9fd" stroke="#d6dbee" strokeWidth="2" />
          <path d="M160 276h20l8 108h-36Z" fill="#2a2f4a" />
          <path d="M112 388Q170 404 228 388" stroke="#f0b94a" strokeWidth="5" strokeLinecap="round" />
          <path d="M168 262l2 20 2-20" stroke="#f0b94a" strokeWidth="3" strokeLinecap="round" />
          <path d="M192 268Q226 282 244 300" stroke="#f8f9fd" strokeWidth="14" strokeLinecap="round" />
          <circle cx="245" cy="302" r="7" fill="#ffe9dc" />
          <path d="M140 222 118 208l22 24ZM200 222l22-14-22 24Z" fill="#ffe9dc" />
          <circle cx="170" cy="216" r="31" fill="#ffe9dc" />
          <path d="M136 210Q140 170 170 168Q200 170 204 210Q186 196 170 204Q154 196 136 210Z" fill="#f4f6fc" stroke="#d6dbee" strokeWidth="1.5" />
          <path d="M140 200Q150 172 170 178Q190 172 200 200Q186 188 170 194Q154 188 140 200Z" fill="#f4f6fc" />
          <ellipse cx="158" cy="222" rx="4.4" ry="6.4" fill="#79d0a2" />
          <ellipse cx="182" cy="222" rx="4.4" ry="6.4" fill="#79d0a2" />
          <circle cx="156.6" cy="219.6" r="1.6" fill="#fff" />
          <circle cx="180.6" cy="219.6" r="1.6" fill="#fff" />
          <path d="M166 237q4 3 8 0" stroke="#d98e8e" strokeWidth="2" strokeLinecap="round" />
          <ellipse cx="148" cy="232" rx="5" ry="3" fill="#ffb7c5" opacity=".5" />
          <ellipse cx="192" cy="232" rx="5" ry="3" fill="#ffb7c5" opacity=".5" />
        </g>

        {/* 黑猫 */}
        <g>
          <path d="M376 400Q426 398 414 352" stroke="#171a28" strokeWidth="10" strokeLinecap="round" />
          <ellipse cx="344" cy="388" rx="36" ry="28" fill="#171a28" />
          <path d="M316 336 320 308l18 14ZM372 336l-4-28-18 14Z" fill="#171a28" />
          <path d="M322 328l1-10 7 5ZM366 328l-1-10-7 5Z" fill="#f7a8b8" opacity=".8" />
          <circle cx="344" cy="350" r="28" fill="#171a28" />
          <ellipse cx="333" cy="350" rx="6.6" ry="7.8" fill="#ffd66e" />
          <ellipse cx="355" cy="350" rx="6.6" ry="7.8" fill="#ffd66e" />
          <ellipse cx="333.3" cy="350.6" rx="2.8" ry="5.8" fill="#171a28" />
          <ellipse cx="354.7" cy="350.6" rx="2.8" ry="5.8" fill="#171a28" />
          <circle cx="331" cy="347" r="2" fill="#fff" />
          <circle cx="353" cy="347" r="2" fill="#fff" />
          <path d="M341.5 360h5l-2.5 3Z" fill="#f7a8b8" />
        </g>

        {/* 光点与花瓣 */}
        {([[286, 150], [214, 120], [270, 236], [300, 200]] as [number, number][]).map(([x, y], i) => (
          <circle key={i} cx={x} cy={y} r={i % 2 ? 2.2 : 3} fill="#e6f7ff" opacity=".85" />
        ))}
        <path d="M440 240c6-4 12 0 10 6-6 2-12-1-10-6ZM90 300c6-4 12 0 10 6-6 2-12-1-10-6ZM470 300c5-3 10 0 8 5-5 2-10-1-8-5Z" fill="#ffd0dc" opacity=".9" />
      </g>
    </svg>
  );
}

/** 页面 404 插画：猫猫占了“0”的位置 */
export function LostArt({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 280 130" fill="none" aria-hidden="true">
      <text x="14" y="112" fontSize="112" fontWeight="800" fill="var(--pt-color-primary-soft)" stroke="var(--pt-color-primary)" strokeWidth="3" strokeLinejoin="round" fontFamily="var(--pt-font)">4</text>
      <text x="170" y="112" fontSize="112" fontWeight="800" fill="var(--pt-color-primary-soft)" stroke="var(--pt-color-primary)" strokeWidth="3" strokeLinejoin="round" fontFamily="var(--pt-font)">4</text>
      <g transform="translate(96 22) scale(1.4)">
        <CatFace size={64} bg={false} />
      </g>
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
