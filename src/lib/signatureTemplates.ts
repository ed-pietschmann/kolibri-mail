/**
 * Signatur-Vorlagen: aus wenigen Angaben (Name, Position, Kontakt, Logo,
 * Akzentfarbe) entsteht mailtaugliches HTML – Tabellen und Inline-Styles,
 * damit es in Outlook, Gmail und Apple Mail gleich aussieht.
 */

export interface SignatureFields {
  greeting: string;
  name: string;
  title: string;
  company: string;
  phone: string;
  mobile: string;
  email: string;
  website: string;
  address: string;
  /** data:-URI (wird beim Senden eingebettet) oder leer */
  logo: string;
  accent: string;
}

export interface SignatureLayout {
  id: string;
  name: string;
  render: (f: SignatureFields) => string;
}

const FONT = "font-family:-apple-system,'Helvetica Neue',Helvetica,Arial,sans-serif";
const SERIF = 'font-family:Georgia,Times,serif';

function esc(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

/** Nur sichere Farbwerte (#rgb/#rrggbb). */
function color(c: string): string {
  return /^#[0-9a-f]{3}([0-9a-f]{3})?$/i.test(c.trim()) ? c.trim() : '#111113';
}

function webHref(url: string): string {
  const u = url.trim();
  return /^https?:\/\//i.test(u) ? u : `https://${u}`;
}

function webLabel(url: string): string {
  return url
    .trim()
    .replace(/^https?:\/\//i, '')
    .replace(/\/$/, '');
}

function telHref(tel: string): string {
  return `tel:${tel.replace(/[^\d+]/g, '')}`;
}

function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  return (
    (parts[0]?.[0] ?? '') + (parts.length > 1 ? (parts.at(-1)?.[0] ?? '') : '')
  ).toUpperCase();
}

/** Kontaktzeilen als Liste [Kürzel, HTML]. */
function contacts(f: SignatureFields, linkColor: string): [string, string][] {
  const out: [string, string][] = [];
  const a = (href: string, text: string) =>
    `<a href="${esc(href)}" style="color:${linkColor};text-decoration:none">${esc(text)}</a>`;
  if (f.phone.trim()) out.push(['T', a(telHref(f.phone), f.phone.trim())]);
  if (f.mobile.trim()) out.push(['M', a(telHref(f.mobile), f.mobile.trim())]);
  if (f.email.trim()) out.push(['E', a(`mailto:${f.email.trim()}`, f.email.trim())]);
  if (f.website.trim()) out.push(['W', a(webHref(f.website), webLabel(f.website))]);
  return out;
}

function roleLine(f: SignatureFields): string {
  return [f.title, f.company]
    .map((s) => s.trim())
    .filter(Boolean)
    .map(esc)
    .join(' · ');
}

function greeting(f: SignatureFields, style = ''): string {
  return f.greeting.trim()
    ? `<p style="margin:0 0 12px;${FONT};font-size:14px;color:#1d1d1f;${style}">${esc(f.greeting.trim())}</p>`
    : '';
}

function logoCell(f: SignatureFields, size: number): string {
  const accent = color(f.accent);
  if (f.logo)
    return `<img src="${f.logo}" width="${size}" alt="${esc(f.company || f.name)}" style="display:block;width:${size}px;height:auto;border:0">`;
  return `<table role="presentation" cellpadding="0" cellspacing="0"><tr><td width="${size}" height="${size}" align="center" valign="middle" style="width:${size}px;height:${size}px;border-radius:${size / 2}px;background:${accent};${FONT};font-size:${Math.round(size * 0.36)}px;font-weight:700;color:#ffffff">${esc(initials(f.name) || '·')}</td></tr></table>`;
}

export const SIGNATURE_LAYOUTS: SignatureLayout[] = [
  {
    id: 'classic',
    name: 'Klassisch',
    render: (f) => {
      const accent = color(f.accent);
      const role = roleLine(f);
      const line = contacts(f, '#6e6e73')
        .map(([, html]) => html)
        .join('<span style="color:#c7c7cc">&nbsp;&nbsp;|&nbsp;&nbsp;</span>');
      return `${greeting(f)}<table role="presentation" cellpadding="0" cellspacing="0" style="${FONT}">
<tr><td style="font-size:16px;font-weight:700;color:${accent};padding:0 0 2px">${esc(f.name)}</td></tr>
${role ? `<tr><td style="font-size:13px;color:#3a3a3f;padding:0 0 8px">${role}</td></tr>` : ''}
${line ? `<tr><td style="font-size:12px;color:#6e6e73">${line}</td></tr>` : ''}
${f.address.trim() ? `<tr><td style="font-size:12px;color:#8e8e93;padding-top:2px">${esc(f.address.trim())}</td></tr>` : ''}
</table>`;
    },
  },
  {
    id: 'logo',
    name: 'Mit Logo',
    render: (f) => {
      const accent = color(f.accent);
      const role = roleLine(f);
      const rows = contacts(f, '#1d1d1f')
        .map(
          ([k, html]) =>
            `<tr><td style="padding:1px 8px 1px 0;font-size:11px;font-weight:700;color:${accent}">${k}</td><td style="padding:1px 0;font-size:12px;color:#1d1d1f">${html}</td></tr>`,
        )
        .join('');
      return `${greeting(f)}<table role="presentation" cellpadding="0" cellspacing="0" style="${FONT}"><tr>
<td valign="top" style="padding:2px 16px 0 0">${logoCell(f, 64)}</td>
<td valign="top" style="padding:0 0 0 16px;border-left:2px solid ${accent}">
<div style="font-size:16px;font-weight:700;color:#111113;line-height:1.3">${esc(f.name)}</div>
${role ? `<div style="font-size:13px;color:#6e6e73;margin:1px 0 8px">${role}</div>` : '<div style="height:6px"></div>'}
${rows ? `<table role="presentation" cellpadding="0" cellspacing="0">${rows}</table>` : ''}
${f.address.trim() ? `<div style="font-size:11px;color:#8e8e93;margin-top:6px">${esc(f.address.trim())}</div>` : ''}
</td></tr></table>`;
    },
  },
  {
    id: 'modern',
    name: 'Modern',
    render: (f) => {
      const accent = color(f.accent);
      const role = roleLine(f);
      const line = contacts(f, '#1d1d1f')
        .map(
          ([k, html]) => `<span style="color:${accent};font-weight:700">${k}</span>&nbsp;${html}`,
        )
        .join('&nbsp;&nbsp;&nbsp;');
      return `${greeting(f)}<table role="presentation" cellpadding="0" cellspacing="0" style="${FONT}">
<tr><td style="font-size:20px;font-weight:800;letter-spacing:-0.02em;color:#111113;line-height:1.2">${esc(f.name)}</td></tr>
<tr><td style="padding:6px 0 8px"><div style="width:40px;height:3px;background:${accent};border-radius:2px;font-size:0;line-height:0">&nbsp;</div></td></tr>
${role ? `<tr><td style="font-size:12px;font-weight:600;letter-spacing:0.06em;text-transform:uppercase;color:#6e6e73;padding-bottom:8px">${role}</td></tr>` : ''}
${line ? `<tr><td style="font-size:12px;color:#1d1d1f">${line}</td></tr>` : ''}
</table>`;
    },
  },
  {
    id: 'card',
    name: 'Karte',
    render: (f) => {
      const accent = color(f.accent);
      const role = roleLine(f);
      const line = contacts(f, accent)
        .map(([, html]) => html)
        .join('<br>');
      return `${greeting(f)}<table role="presentation" cellpadding="0" cellspacing="0" style="${FONT};background:#f5f5f7;border-radius:12px"><tr>
<td valign="middle" style="padding:16px 14px 16px 16px">${logoCell(f, 48)}</td>
<td valign="middle" style="padding:16px 20px 16px 0">
<div style="font-size:15px;font-weight:700;color:#111113">${esc(f.name)}</div>
${role ? `<div style="font-size:12px;color:#6e6e73;margin-top:1px">${role}</div>` : ''}
</td>
${line ? `<td valign="middle" style="padding:16px 18px;border-left:1px solid #e3e3e8;font-size:12px;line-height:1.6">${line}</td>` : ''}
</tr></table>`;
    },
  },
  {
    id: 'personal',
    name: 'Persönlich',
    render: (f) => {
      const accent = color(f.accent);
      const line = contacts(f, '#8e8e93')
        .map(([, html]) => html)
        .join('&nbsp;&nbsp;·&nbsp;&nbsp;');
      return `${f.greeting.trim() ? `<p style="margin:0 0 6px;${SERIF};font-style:italic;font-size:15px;color:#3a3a3f">${esc(f.greeting.trim())}</p>` : ''}
<p style="margin:0;${SERIF};font-size:22px;color:${accent};line-height:1.2">${esc(f.name)}</p>
${roleLine(f) ? `<p style="margin:4px 0 0;${FONT};font-size:12px;color:#6e6e73">${roleLine(f)}</p>` : ''}
${line ? `<p style="margin:6px 0 0;${FONT};font-size:12px;color:#8e8e93">${line}</p>` : ''}`;
    },
  },
  {
    id: 'minimal',
    name: 'Schlicht',
    render: (f) => {
      const parts = [
        `<b style="color:#1d1d1f">${esc(f.name)}</b>`,
        roleLine(f),
        ...contacts(f, '#6e6e73').map(([, html]) => html),
      ].filter(Boolean);
      return `${greeting(f)}<p style="margin:0;${FONT};font-size:13px;line-height:1.6;color:#6e6e73">${parts.join('<br>')}</p>`;
    },
  },
];

/** Signatur-HTML für die Vorschau als ganze Seite. */
export function signaturePage(html: string): string {
  return `<body style="margin:0;padding:16px 18px;background:#fff;${FONT};font-size:14px;color:#1d1d1f">${html}</body>`;
}
