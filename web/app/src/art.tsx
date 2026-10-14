// 品牌标志与插画（内联 SVG，颜色取主题令牌，亮 / 暗主题自动适配）。

/** 站标：深夜蓝底、金色魔法阵、翡翠宝石的法杖顶 */
export function Logo({ size = 32 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 64 64" aria-hidden="true">
      <defs>
        <linearGradient id="logo-bg" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#2b3a63" />
          <stop offset="1" stopColor="#1b2440" />
        </linearGradient>
      </defs>
      <rect width="64" height="64" rx="16" fill="url(#logo-bg)" />
      <circle cx="32" cy="33" r="22" fill="none" stroke="#d8b56a" strokeWidth="1.6" opacity=".9" />
      <circle cx="32" cy="33" r="17.5" fill="none" stroke="#d8b56a" strokeWidth="1" strokeDasharray="2 3" opacity=".7" />
      <path d="M32 55V28" stroke="#ead7a6" strokeWidth="3" strokeLinecap="round" />
      <path d="M21.5 20.5Q32 37 42.5 20.5" fill="none" stroke="#d8b56a" strokeWidth="3.2" strokeLinecap="round" />
      <circle cx="32" cy="22.5" r="5.4" fill="#3fb59b" />
      <circle cx="30.4" cy="20.8" r="1.7" fill="#e9fff8" />
      <path d="M48 12l1.2 2.8 2.8 1.2-2.8 1.2L48 20l-1.2-2.8-2.8-1.2 2.8-1.2Z" fill="#f4e6c2" />
    </svg>
  );
}

/** 首页 / 登录页主插画：晴空、远山、苍月草花海与旅途中的魔法使（原创） */
export function HeroArt({ className }: { className?: string }) {
  const flowers: [number, number, string][] = [
    [34, 396, '#6f9fd8'], [70, 414, '#9cc0ef'], [110, 400, '#fff'], [150, 424, '#6f9fd8'], [262, 404, '#9cc0ef'], [298, 420, '#6f9fd8'],
    [214, 426, '#fff'], [336, 400, '#6f9fd8'], [372, 424, '#9cc0ef'], [410, 402, '#fff'], [446, 418, '#6f9fd8'], [486, 398, '#9cc0ef'],
    [20, 428, '#9cc0ef'], [500, 428, '#6f9fd8'], [240, 388, '#6f9fd8'], [470, 380, '#9cc0ef'],
  ];
  return (
    <svg className={className} viewBox="0 0 520 440" fill="none" role="img" aria-hidden="true">
      <defs>
        <linearGradient id="ha-sky" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#8fbde8" />
          <stop offset=".6" stopColor="#cfe3f5" />
          <stop offset="1" stopColor="#f6efe0" />
        </linearGradient>
        <linearGradient id="ha-field" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#9fd0a0" />
          <stop offset="1" stopColor="#6fae86" />
        </linearGradient>
        <radialGradient id="ha-sun" cx=".5" cy=".5" r=".5">
          <stop offset="0" stopColor="#fffaf0" stopOpacity=".95" />
          <stop offset="1" stopColor="#fffaf0" stopOpacity="0" />
        </radialGradient>
        <radialGradient id="ha-orb" cx=".5" cy=".5" r=".5">
          <stop offset="0" stopColor="#c9fff0" />
          <stop offset="1" stopColor="#c9fff0" stopOpacity="0" />
        </radialGradient>
        <clipPath id="ha-clip">
          <rect width="520" height="440" rx="32" />
        </clipPath>
      </defs>
      <g clipPath="url(#ha-clip)">
        <rect width="520" height="440" fill="url(#ha-sky)" />
        <circle cx="410" cy="96" r="110" fill="url(#ha-sun)" />
        {/* 云 */}
        <g fill="#fff" opacity=".92">
          <ellipse cx="96" cy="96" rx="58" ry="18" />
          <ellipse cx="128" cy="82" rx="34" ry="20" />
          <ellipse cx="74" cy="86" rx="24" ry="14" />
          <ellipse cx="350" cy="58" rx="46" ry="13" />
          <ellipse cx="376" cy="48" rx="26" ry="14" />
          <ellipse cx="470" cy="180" rx="40" ry="11" opacity=".8" />
        </g>
        {/* 远山 */}
        <path d="M0 280 70 214l52 40 64-70 74 76 58-46 72 58 54-38 76 56v210H0Z" fill="#a9c2e2" />
        <path d="M0 300 90 252l70 34 80-44 92 56 70-30 118 40v132H0Z" fill="#8fb0d6" opacity=".85" />
        {/* 花海 */}
        <path d="M0 336C110 304 210 326 300 330S452 316 520 334V440H0Z" fill="url(#ha-field)" />
        <path d="M0 372C120 352 240 368 340 372S470 364 520 374V440H0Z" fill="#6aa883" opacity=".75" />
        {flowers.map(([x, y, c], i) => (
          <g key={i}>
            <path d={`M${x} ${y + 2}v12`} stroke="#4f8f6d" strokeWidth="1.4" />
            {[0, 72, 144, 216, 288].map((r) => (
              <ellipse key={r} cx={x} cy={y - 4.5} rx="2.7" ry="4.4" fill={c} transform={`rotate(${r} ${x} ${y})`} />
            ))}
            <circle cx={x} cy={y} r="1.8" fill="#f2d48a" />
          </g>
        ))}

        {/* 魔法阵 */}
        <g transform="translate(172 262)" stroke="#d8b56a" fill="none" opacity=".75">
          <ellipse rx="96" ry="24" strokeWidth="1.6" />
          <ellipse rx="78" ry="19" strokeWidth="1" strokeDasharray="3 5" />
          <ellipse rx="56" ry="13" strokeWidth="1.2" />
        </g>

        {/* 魔法使 */}
        <g>
          <path d="M244 392 254 214" stroke="#8a6a46" strokeWidth="5.5" strokeLinecap="round" />
          <circle cx="252" cy="196" r="34" fill="url(#ha-orb)" />
          <path d="M232 186q20 26 40 0" stroke="#d8b56a" strokeWidth="4" strokeLinecap="round" fill="none" />
          <circle cx="252" cy="192" r="9" fill="#3fb59b" stroke="#f2e2b4" strokeWidth="2" />
          <circle cx="249" cy="189" r="2.6" fill="#e9fff8" />
          <path d="M126 150c-22 44-14 120-2 168" stroke="#f3f5fb" strokeWidth="16" strokeLinecap="round" />
          <path d="M214 150c22 44 14 120 2 168" stroke="#f3f5fb" strokeWidth="16" strokeLinecap="round" />
          <path d="M150 262Q170 246 190 262L228 388Q170 404 112 388Z" fill="#fbfcff" stroke="#d6dbee" strokeWidth="2" />
          <path d="M160 276h20l8 108h-36Z" fill="#26314f" />
          <path d="M112 388Q170 404 228 388" stroke="#c9a45c" strokeWidth="5" strokeLinecap="round" />
          <path d="M150 262Q170 276 190 262" stroke="#c9a45c" strokeWidth="3" strokeLinecap="round" fill="none" />
          <path d="M192 268Q226 282 248 300" stroke="#fbfcff" strokeWidth="14" strokeLinecap="round" />
          <circle cx="249" cy="302" r="7" fill="#ffe9dc" />
          <path d="M140 222 118 208l22 24ZM200 222l22-14-22 24Z" fill="#ffe9dc" />
          <circle cx="139" cy="226" r="2.4" fill="#d9534f" />
          <circle cx="201" cy="226" r="2.4" fill="#d9534f" />
          <circle cx="170" cy="216" r="31" fill="#ffe9dc" />
          <path d="M136 210Q140 170 170 168Q200 170 204 210Q186 196 170 204Q154 196 136 210Z" fill="#f4f6fc" stroke="#d6dbee" strokeWidth="1.5" />
          <path d="M140 200Q150 172 170 178Q190 172 200 200Q186 188 170 194Q154 188 140 200Z" fill="#f4f6fc" />
          <ellipse cx="158" cy="222" rx="4.2" ry="6" fill="#3f9a7f" />
          <ellipse cx="182" cy="222" rx="4.2" ry="6" fill="#3f9a7f" />
          <circle cx="156.8" cy="219.8" r="1.5" fill="#fff" />
          <circle cx="180.8" cy="219.8" r="1.5" fill="#fff" />
          <path d="M166 237q4 2 8 0" stroke="#c98a8a" strokeWidth="2" strokeLinecap="round" />
        </g>

        {/* 光点与花瓣 */}
        {([[290, 150], [214, 120], [276, 236], [306, 204], [330, 168]] as [number, number][]).map(([x, y], i) => (
          <path key={i} d={`M${x} ${y - 5}l1.5 3.5 3.5 1.5-3.5 1.5L${x} ${y + 5}l-1.5-3.5-3.5-1.5 3.5-1.5Z`} fill="#f4e6c2" />
        ))}
        <path d="M440 250c6-4 12 0 10 6-6 2-12-1-10-6ZM90 300c6-4 12 0 10 6-6 2-12-1-10-6ZM470 300c5-3 10 0 8 5-5 2-10-1-8-5ZM380 220c5-3 10 0 8 5-5 2-10-1-8-5Z" fill="#9cc0ef" />
      </g>
    </svg>
  );
}

/** 404：魔法阵中间的数字 */
export function LostArt({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 280 150" fill="none" aria-hidden="true">
      <g transform="translate(140 80)" stroke="var(--pt-color-gold)" fill="none">
        <circle r="62" strokeWidth="1.6" />
        <circle r="52" strokeWidth="1" strokeDasharray="3 4" />
        <path d="M0-62 54 31H-54Z" strokeWidth="1" opacity=".6" />
        <path d="M0 62-54-31H54Z" strokeWidth="1" opacity=".6" />
      </g>
      <text x="140" y="96" textAnchor="middle" fontSize="44" fontWeight="700" fill="var(--pt-color-fg)" fontFamily="var(--pt-font-display)">404</text>
      <path d="M36 128c6-4 12 0 10 6-6 2-12-1-10-6ZM236 30c6-4 12 0 10 6-6 2-12-1-10-6Z" fill="var(--pt-color-sky)" />
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
  compass: 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18Zm3.5-12.5-2 5-5 2 2-5Z',
  book: 'M4 5.5C4 4.7 4.7 4 5.5 4H11v16H5.5A1.5 1.5 0 0 1 4 18.5ZM20 5.5c0-.8-.7-1.5-1.5-1.5H13v16h5.5c.8 0 1.5-.7 1.5-1.5Z',
  help: 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18Zm-2.5-11.5a2.5 2.5 0 1 1 3.5 2.3c-.6.3-1 .8-1 1.5V14m0 3h.01',
  filter: 'M4 6h16M7 12h10M10 18h4',
  plus: 'M12 5v14M5 12h14',
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
