# 内置头像生成器：《葬送的芙莉莲》主要人物的原创矢量头像（同一套部件模板，按人物参数组合）。
# 用法：python3 web/tools/avatars.py → 写 web/assets/avatars/<slug>.svg 与 index.json
import json
import os
import sys

OUT = os.path.join(os.path.dirname(__file__), '..', 'assets', 'avatars')

SKIN = '#fbe6d8'
SKIN_SH = '#efc4ae'
LINE = '#3a2a2a'


def shade(hex_color, f):
    """f < 1 变暗，f > 1 变亮"""
    h = hex_color.lstrip('#')
    r, g, b = (int(h[i:i + 2], 16) for i in (0, 2, 4))
    if f < 1:
        r, g, b = (int(c * f) for c in (r, g, b))
    else:
        r, g, b = (int(c + (255 - c) * (f - 1)) for c in (r, g, b))
    return f'#{r:02x}{g:02x}{b:02x}'


def defs(c):
    s = c['slug']
    return f'''<defs>
<radialGradient id="bg-{s}" cx="50%" cy="38%" r="75%"><stop offset="0" stop-color="{shade(c['bg'], 1.35)}"/><stop offset="1" stop-color="{shade(c['bg'], 0.7)}"/></radialGradient>
<linearGradient id="hair-{s}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{shade(c['hair'], 1.18)}"/><stop offset="1" stop-color="{shade(c['hair'], 0.82)}"/></linearGradient>
<linearGradient id="iris-{s}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{shade(c['eye'], 0.45)}"/><stop offset=".55" stop-color="{c['eye']}"/><stop offset="1" stop-color="{shade(c['eye'], 1.45)}"/></linearGradient>
<clipPath id="clip-{s}"><rect width="256" height="256"/></clipPath>
</defs>'''


def background(c):
    s = c['slug']
    dots = ''.join(
        f'<circle cx="{x}" cy="{y}" r="{r}" fill="#fff" opacity="{o}"/>'
        for x, y, r, o in [(30, 40, 2.2, .7), (218, 52, 1.6, .6), (204, 30, 3, .45), (46, 120, 1.4, .5), (226, 140, 2, .5), (20, 196, 1.8, .4), (236, 210, 1.4, .4)]
    )
    return f'<rect width="256" height="256" fill="url(#bg-{s})"/>{dots}'


def body(c):
    cl = c['cloth']
    trim = c.get('trim', '#d9b56a')
    out = f'<path d="M28 256 C34 214 74 194 128 194 C182 194 222 214 228 256 Z" fill="{cl}"/>'
    col = c.get('collar', 'plain')
    if col == 'frieren':
        out += f'<path d="M96 192 L160 192 L166 214 L90 214 Z" fill="#f6f2ea" stroke="{trim}" stroke-width="3"/>'
        out += f'<circle cx="128" cy="203" r="5" fill="#b3232f" stroke="{trim}" stroke-width="2"/>'
        out += f'<path d="M112 214 L106 256 M144 214 L150 256" stroke="{trim}" stroke-width="7"/>'
        out += '<path d="M116 222 L140 222 L144 256 L112 256 Z" fill="#2b2f3a"/><path d="M114 234 H142 M113 246 H143" stroke="#e9e6ee" stroke-width="5"/>'
    elif col == 'fern':
        out += '<path d="M92 194 L128 228 L164 194 L172 206 L128 246 L84 206 Z" fill="#f4f1ec"/>'
        out += '<path d="M120 226 L128 242 L136 226 Z" fill="#7a2836"/>'
    elif col == 'cape':
        out += f'<path d="M28 256 C34 220 64 202 96 198 L128 256 Z M228 256 C222 220 192 202 160 198 L128 256 Z" fill="{shade(cl, 0.8)}"/>'
        out += f'<path d="M100 196 L128 220 L156 196" fill="none" stroke="{trim}" stroke-width="5"/>'
        out += f'<circle cx="128" cy="222" r="6" fill="{trim}"/>'
    elif col == 'priest':
        out += f'<path d="M98 194 L158 194 L150 210 L106 210 Z" fill="{trim}"/>'
        out += f'<path d="M122 210 H134 V256 H122 Z" fill="{trim}" opacity=".9"/><path d="M114 228 H142" stroke="{trim}" stroke-width="6"/>'
    elif col == 'armor':
        out += f'<path d="M40 256 C46 222 80 204 128 204 C176 204 210 222 216 256 Z" fill="{shade(cl, 1.25)}" stroke="{shade(cl, 0.6)}" stroke-width="3"/>'
        out += f'<path d="M128 206 V256" stroke="{shade(cl, 0.6)}" stroke-width="3"/>'
    elif col == 'mage':
        out += f'<path d="M100 194 L128 214 L156 194 L162 204 L128 228 L94 204 Z" fill="{trim}"/>'
    else:
        out += f'<path d="M102 194 Q128 212 154 194" fill="none" stroke="{shade(cl, 0.7)}" stroke-width="4"/>'
    return out


def neck():
    return f'<path d="M112 160 H144 V200 Q128 210 112 200 Z" fill="{SKIN_SH}"/>'


def ears(c):
    if c.get('elf'):
        return (f'<path d="M84 122 C70 116 50 100 40 92 C52 112 62 134 86 146 Z" fill="{SKIN}" stroke="{SKIN_SH}" stroke-width="2"/>'
                f'<path d="M172 122 C186 116 206 100 216 92 C204 112 194 134 170 146 Z" fill="{SKIN}" stroke="{SKIN_SH}" stroke-width="2"/>'
                f'<path d="M80 128 C68 122 58 112 52 106" stroke="{SKIN_SH}" stroke-width="3" fill="none"/>'
                f'<path d="M176 128 C188 122 198 112 204 106" stroke="{SKIN_SH}" stroke-width="3" fill="none"/>')
    return (f'<ellipse cx="82" cy="134" rx="8" ry="12" fill="{SKIN}" stroke="{SKIN_SH}" stroke-width="2"/>'
            f'<ellipse cx="174" cy="134" rx="8" ry="12" fill="{SKIN}" stroke="{SKIN_SH}" stroke-width="2"/>')


def earrings(c):
    if c.get('earrings'):
        col = c['earrings']
        return ''.join(
            f'<circle cx="{x}" cy="150" r="3" fill="#d9b56a"/><path d="M{x} 152 C{x - 5} 162 {x - 4} 172 {x} 174 C{x + 4} 172 {x + 5} 162 {x} 152 Z" fill="{col}" stroke="{shade(col, 0.6)}" stroke-width="1.2"/>'
            for x in (82, 174)
        )
    return ''


def face(c):
    chin = c.get('chin', 176)
    return f'<path d="M82 100 C80 140 98 {chin - 8} 128 {chin} C158 {chin - 8} 176 140 174 100 Z" fill="{SKIN}"/>'


def eyes(c):
    s = c['slug']
    lash = c.get('lash', LINE)
    style = c.get('eyes', 'round')
    out = ''
    for cx, side in ((106, -1), (150, 1)):
        if style == 'closed':
            out += f'<path d="M{cx - 12} 132 Q{cx} 140 {cx + 12} 132" fill="none" stroke="{lash}" stroke-width="3.5" stroke-linecap="round"/>'
            continue
        ry = 11 if style == 'round' else 8
        cy = 132 if style == 'round' else 134
        out += f'<ellipse cx="{cx}" cy="{cy}" rx="10" ry="{ry + 1}" fill="#fff"/>'
        out += f'<ellipse cx="{cx}" cy="{cy + 1}" rx="8" ry="{ry}" fill="url(#iris-{s})"/>'
        out += f'<ellipse cx="{cx}" cy="{cy + 1}" rx="3.6" ry="{ry * 0.5:.1f}" fill="{shade(c["eye"], 0.3)}"/>'
        out += f'<circle cx="{cx - 3}" cy="{cy - 4}" r="2.8" fill="#fff"/><circle cx="{cx + 3}" cy="{cy + 5}" r="1.3" fill="#fff" opacity=".8"/>'
        top = cy - ry - 1
        ox = cx + 13 * side
        out += (f'<path d="M{cx - 12 * side} {top + 5} Q{cx} {top - 4} {ox} {top + 3} L{ox + 3 * side} {top + 1}" fill="none" '
                f'stroke="{lash}" stroke-width="3.6" stroke-linecap="round" stroke-linejoin="round"/>')
        out += f'<path d="M{cx - 6} {cy + ry + 2} Q{cx} {cy + ry + 4} {cx + 7} {cy + ry + 1}" fill="none" stroke="{lash}" stroke-width="1.2" opacity=".55"/>'
    if c.get('glasses'):
        g = c['glasses']
        out += (f'<g fill="none" stroke="{g}" stroke-width="3"><rect x="90" y="118" width="32" height="28" rx="9"/>'
                f'<rect x="134" y="118" width="32" height="28" rx="9"/><path d="M122 130 Q128 126 134 130"/></g>')
    return out


def brows(c):
    col = shade(c.get('brow', c['hair']), 0.6)
    tilt = c.get('brow_tilt', 0)
    y = 108
    return (f'<path d="M94 {y + tilt} Q106 {y - 5} 118 {y - tilt}" fill="none" stroke="{col}" stroke-width="3" stroke-linecap="round"/>'
            f'<path d="M138 {y - tilt} Q150 {y - 5} 162 {y + tilt}" fill="none" stroke="{col}" stroke-width="3" stroke-linecap="round"/>')


def nose_mouth(c):
    m = c.get('mouth', 'smile')
    out = f'<path d="M128 146 L126 151" stroke="{SKIN_SH}" stroke-width="2.4" stroke-linecap="round"/>'
    if m == 'smile':
        out += '<path d="M118 160 Q128 166 138 160" fill="none" stroke="#a5524a" stroke-width="2.6" stroke-linecap="round"/>'
    elif m == 'flat':
        out += '<path d="M120 161 Q128 163 136 161" fill="none" stroke="#a5524a" stroke-width="2.4" stroke-linecap="round"/>'
    elif m == 'smirk':
        out += '<path d="M118 161 Q130 164 140 156" fill="none" stroke="#a5524a" stroke-width="2.6" stroke-linecap="round"/>'
    elif m == 'grin':
        out += '<path d="M114 157 Q128 172 142 157 Z" fill="#8c3a3a"/><path d="M118 158 H138" stroke="#fff" stroke-width="3"/>'
    elif m == 'pout':
        out += '<path d="M122 162 Q128 158 134 162" fill="none" stroke="#a5524a" stroke-width="2.4" stroke-linecap="round"/>'
    if c.get('blush', True):
        out += '<ellipse cx="96" cy="152" rx="9" ry="5" fill="#f08a8a" opacity=".3"/><ellipse cx="160" cy="152" rx="9" ry="5" fill="#f08a8a" opacity=".3"/>'
    return out


def hair_back(c):
    s = c['slug']
    st = c['style']
    f = f'url(#hair-{s})'
    if st in ('long', 'fern', 'flamme', 'methode'):
        return f'<path d="M70 96 C58 150 62 214 52 256 L204 256 C194 214 198 150 186 96 Z" fill="{f}"/>'
    if st == 'frieren':
        return (f'<path d="M74 96 C64 140 66 190 70 220 L186 220 C190 190 192 140 182 96 Z" fill="{f}"/>'
                f'<path d="M78 74 C50 92 40 150 44 210 C46 236 40 252 36 256 L76 256 C70 220 74 170 86 110 Z" fill="{f}"/>'
                f'<path d="M178 74 C206 92 216 150 212 210 C210 236 216 252 220 256 L180 256 C186 220 182 170 170 110 Z" fill="{f}"/>')
    if st == 'serie':
        return (f'<path d="M70 96 C58 150 62 214 52 256 L204 256 C194 214 198 150 186 96 Z" fill="{f}"/>')
    if st == 'ubel':
        return f'<path d="M176 92 C204 120 210 180 196 256 L160 256 C176 200 172 140 160 100 Z" fill="{f}"/>'
    if st == 'lawine':
        return (f'<circle cx="74" cy="72" r="22" fill="{f}"/><circle cx="182" cy="72" r="22" fill="{f}"/>')
    return ''


def hair_front(c):
    s = c['slug']
    st = c['style']
    f = f'url(#hair-{s})'
    sh = shade(c['hair'], 0.72)
    dome = f'<path d="M76 118 C70 64 104 46 128 46 C152 46 186 64 180 118 Z" fill="{f}"/>'
    # 刘海：锯齿下沿
    fringe_pts = {
        'default': 'M78 120 L80 86 C92 56 164 56 176 86 L178 120 L168 98 L160 116 L150 92 L138 114 L128 90 L118 114 L106 92 L96 116 L88 98 Z',
        'blunt': 'M80 118 L80 84 C92 56 164 56 176 84 L176 118 L170 112 L160 114 L150 111 L140 113 L128 111 L116 113 L106 111 L96 114 L86 112 Z',
        'parted': 'M78 122 L80 84 C92 56 164 56 176 84 L178 122 L170 100 L156 88 L136 80 L128 78 L132 92 L120 80 L104 88 L90 100 Z',
        'swept': 'M78 120 L80 84 C92 56 164 56 176 84 L178 110 L164 96 L148 104 L132 92 L120 104 L104 96 L92 108 Z',
    }
    out = ''
    if st == 'stark':
        out += (f'<path d="M74 122 L68 92 L58 80 L78 78 L72 56 L96 64 L100 40 L118 56 L132 34 L142 56 L162 40 L164 64 L188 58 L180 80 L198 84 L184 96 L182 122 '
                f'L170 98 L160 116 L148 92 L138 112 L126 90 L116 112 L104 92 L94 114 L86 98 Z" fill="{f}"/>')
        return out
    if st == 'eisen':
        # 带角头盔 + 大胡子
        out += (f'<path d="M72 112 C70 60 100 42 128 42 C156 42 186 60 184 112 Z" fill="#8b8f99" stroke="#5a5e68" stroke-width="3"/>'
                f'<path d="M72 104 H184 V116 H72 Z" fill="#6d717c"/>'
                f'<path d="M76 80 C54 76 38 56 36 34 C50 52 62 58 82 62 Z" fill="#efe6cf" stroke="#b9ab86" stroke-width="2"/>'
                f'<path d="M180 80 C202 76 218 56 220 34 C206 52 194 58 174 62 Z" fill="#efe6cf" stroke="#b9ab86" stroke-width="2"/>')
        return out
    if st == 'denken':
        out += (f'<path d="M78 124 C72 96 78 78 92 72 L96 100 Z" fill="{f}"/><path d="M178 124 C184 96 178 78 164 72 L160 100 Z" fill="{f}"/>'
                f'<path d="M92 72 C104 62 152 62 164 72 C150 68 106 68 92 72 Z" fill="{f}" opacity=".7"/>')
        return out
    out += dome
    out += f'<path d="{fringe_pts[c.get("fringe", "default")]}" fill="{f}"/>'
    out += f'<path d="M100 66 C112 58 132 56 148 62" fill="none" stroke="#fff" stroke-width="4" stroke-linecap="round" opacity=".35"/>'
    # 侧发
    side = c.get('side', 'medium')
    if side == 'long':
        out += (f'<path d="M80 100 C72 140 74 186 66 220 C80 200 86 160 90 120 Z" fill="{f}"/>'
                f'<path d="M176 100 C184 140 182 186 190 220 C176 200 170 160 166 120 Z" fill="{f}"/>')
    elif side == 'medium':
        out += (f'<path d="M80 100 C74 130 78 156 74 176 C88 162 90 140 92 118 Z" fill="{f}"/>'
                f'<path d="M176 100 C182 130 178 156 182 176 C168 162 166 140 164 118 Z" fill="{f}"/>')
    elif side == 'short':
        out += (f'<path d="M80 100 C76 118 78 132 80 144 C88 132 90 120 92 112 Z" fill="{f}"/>'
                f'<path d="M176 100 C180 118 178 132 176 144 C168 132 166 120 164 112 Z" fill="{f}"/>')
    if st == 'frieren':
        out += (f'<path d="M72 86 C80 80 84 76 86 70" stroke="{sh}" stroke-width="5" fill="none"/>'
                f'<path d="M184 86 C176 80 172 76 170 70" stroke="{sh}" stroke-width="5" fill="none"/>'
                '<rect x="70" y="82" width="9" height="6" rx="2" fill="#3a3f4c"/><rect x="177" y="82" width="9" height="6" rx="2" fill="#3a3f4c"/>')
    if st == 'fern':
        out += '<path d="M160 70 l8 -6 l4 8 l-8 4 Z" fill="#f4f1ec"/>'
    if st == 'ubel':
        out += f'<path d="M168 84 C182 90 186 98 184 108" stroke="{sh}" stroke-width="4" fill="none"/><circle cx="182" cy="96" r="5" fill="#2b2f3a"/>'
    if st == 'serie':
        out += ''.join(f'<ellipse cx="{x}" cy="{y}" rx="10" ry="8" fill="{f}" stroke="{sh}" stroke-width="1.5"/>'
                       for x, y in ((60, 196), (58, 212), (56, 228), (54, 244)))
        out += f'<path d="M64 120 C58 150 62 176 60 192" stroke="url(#hair-{s})" stroke-width="16" fill="none"/>'
    if st == 'flamme':
        out += '<circle cx="164" cy="72" r="6" fill="#f2d27a"/><circle cx="164" cy="72" r="3" fill="#c0392b"/>'
    if st == 'lawine':
        out += f'<path d="M58 72 C64 56 84 52 92 64" stroke="{sh}" stroke-width="3" fill="none"/><path d="M198 72 C192 56 172 52 164 64" stroke="{sh}" stroke-width="3" fill="none"/>'
    return out


def beard(c):
    b = c.get('beard')
    if not b:
        return ''
    if b == 'full':
        col = c.get('beard_color', '#d8cfb6')
        return (f'<path d="M84 140 C86 186 104 222 128 228 C152 222 170 186 172 140 C162 160 150 168 128 168 C106 168 94 160 84 140 Z" fill="{col}" stroke="{shade(col, 0.75)}" stroke-width="2"/>'
                f'<path d="M106 158 C114 150 124 152 128 156 C132 152 142 150 150 158 C140 164 132 162 128 160 C124 162 116 164 106 158 Z" fill="{shade(col, 0.9)}"/>')
    if b == 'moustache':
        col = c.get('beard_color', '#eeeeee')
        return (f'<path d="M108 158 C116 150 124 152 128 156 C132 152 140 150 148 158 C140 162 132 160 128 158 C124 160 116 162 108 158 Z" fill="{col}"/>'
                f'<path d="M116 170 C120 182 136 182 140 170 C134 174 122 174 116 170 Z" fill="{col}"/>')
    if b == 'goatee':
        col = shade(c['hair'], 0.8)
        return (f'<path d="M116 166 C120 180 136 180 140 166 C134 170 122 170 116 166 Z" fill="{col}"/>'
                f'<path d="M110 156 C118 152 124 154 128 156 C132 154 138 152 146 156" stroke="{col}" stroke-width="3" fill="none"/>')
    return ''


def wrinkles(c):
    if not c.get('old'):
        return ''
    return (f'<path d="M92 144 Q96 150 94 156 M164 144 Q160 150 162 156" stroke="{SKIN_SH}" stroke-width="2" fill="none"/>'
            f'<path d="M110 98 Q128 94 146 98" stroke="{SKIN_SH}" stroke-width="2" fill="none"/>')


def avatar(c):
    s = c['slug']
    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="36 30 184 184" width="256" height="256"><title>{c["name"]}</title>',
        defs(c),
        f'<g clip-path="url(#clip-{s})">',
        background(c),
        hair_back(c),
        body(c),
        neck(),
        ears(c),
        face(c),
        wrinkles(c),
        eyes(c),
        brows(c),
        nose_mouth(c),
        beard(c),
        hair_front(c),
        earrings(c),
        '</g></svg>',
    ]
    return ''.join(parts)


CHARACTERS = [
    dict(slug='frieren', name='芙莉莲', hair='#e4e1ec', eye='#3f9a78', bg='#2c4f86', cloth='#f3efe6', collar='frieren', elf=True,
         style='frieren', fringe='parted', side='long', earrings='#c0263a', mouth='flat', eyes='half', brow='#b9b2c6'),
    dict(slug='fern', name='菲伦', hair='#6a4a90', eye='#7a55b0', bg='#5b4a86', cloth='#24222c', collar='fern', style='fern', fringe='blunt',
         side='long', mouth='pout', eyes='half'),
    dict(slug='stark', name='修塔尔克', hair='#c0472f', eye='#b8402e', bg='#8a5a2a', cloth='#4a4f5a', collar='plain', style='stark',
         mouth='grin', brow_tilt=-2),
    dict(slug='himmel', name='辛美尔', hair='#7ea8dc', eye='#3f78c6', bg='#3a6aa8', cloth='#f1eee8', collar='cape', trim='#4a78c0',
         style='short', fringe='swept', side='short', mouth='smile'),
    dict(slug='heiter', name='海塔', hair='#b49a74', eye='#6a5a3e', bg='#7a6a4a', cloth='#f3f0e8', collar='priest', trim='#c9a24f',
         style='short', fringe='parted', side='medium', mouth='smile', eyes='closed'),
    dict(slug='eisen', name='艾泽', hair='#d8cfb6', eye='#5a4a3a', bg='#6a4a2c', cloth='#6a5040', collar='armor', style='eisen',
         beard='full', beard_color='#ddd2b4', mouth='flat', eyes='half', brow='#cfc3a2', brow_tilt=-3, chin=170),
    dict(slug='sein', name='赞恩', hair='#3f7480', eye='#3a7a84', bg='#2e5a5c', cloth='#2a3640', collar='priest', trim='#c9a24f',
         style='short', fringe='swept', side='short', beard='goatee', mouth='flat', eyes='half'),
    dict(slug='ubel', name='尤贝尔', hair='#2f5c48', eye='#4aa07a', bg='#2c4a3a', cloth='#2a2a30', collar='mage', trim='#6a8a7a',
         style='ubel', fringe='default', side='medium', mouth='smirk', eyes='half', brow_tilt=2),
    dict(slug='land', name='兰托', hair='#30323e', eye='#5a6474', bg='#3a4a5c', cloth='#e9e7e2', collar='plain', style='short',
         fringe='blunt', side='short', glasses='#2a2a33', mouth='flat', eyes='half'),
    dict(slug='denken', name='邓肯', hair='#e8e6e2', eye='#4a4a52', bg='#6a2a34', cloth='#2c2a34', collar='mage', trim='#b28a4a',
         style='denken', beard='moustache', beard_color='#efede8', mouth='flat', eyes='half', old=True, brow='#d8d6d0', blush=False, brow_tilt=-3),
    dict(slug='serie', name='赛莉耶', hair='#e9c86a', eye='#c9962e', bg='#5a4a2a', cloth='#f3efe6', collar='mage', trim='#c9a24f', elf=True,
         style='serie', fringe='parted', side='medium', mouth='smirk', eyes='half'),
    dict(slug='flamme', name='芙兰梅', hair='#d9774a', eye='#3f8a6e', bg='#8a3a2a', cloth='#f3efe6', collar='mage', trim='#c9a24f', elf=True,
         style='flamme', fringe='parted', side='long', mouth='smile'),
    dict(slug='methode', name='梅特黛', hair='#b7a4d6', eye='#7a63b0', bg='#4a3a6a', cloth='#2c2a3a', collar='priest', trim='#c9a24f',
         style='methode', fringe='swept', side='long', mouth='smile'),
    dict(slug='lawine', name='拉维涅', hair='#c8743e', eye='#7a4a2a', bg='#7a4a2a', cloth='#3a4a6a', collar='mage', trim='#d9b56a',
         style='lawine', fringe='default', side='short', mouth='smirk', brow_tilt=2),
]


def main():
    os.makedirs(OUT, exist_ok=True)
    index = []
    for c in CHARACTERS:
        with open(os.path.join(OUT, f'{c["slug"]}.svg'), 'w', encoding='utf-8') as fh:
            fh.write(avatar(c))
        index.append({'slug': c['slug'], 'name': c['name']})
    with open(os.path.join(OUT, 'index.json'), 'w', encoding='utf-8') as fh:
        json.dump(index, fh, ensure_ascii=False)
    print(f'{len(index)} avatars → {os.path.normpath(OUT)}', file=sys.stderr)


if __name__ == '__main__':
    main()
