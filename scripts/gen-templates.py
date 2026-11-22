#!/usr/bin/env python3
"""Erzeugt crates/store/migrations/011_designs.sql aus den Design-Vorlagen unten.

Die Vorlagen sind mailtauglich: Tabellen-Layout, nur Inline-Styles, max. 600 px,
Systemschriften. Platzhalter: {{inhalt}}, {{name}}, {{email}}, {{betreff}}.
Aufruf: python3 -I scripts/gen-templates.py
"""

from pathlib import Path

SANS = "-apple-system,BlinkMacSystemFont,Helvetica,Arial,sans-serif"
SERIF = "Georgia,Times,serif"


def page(bg: str, inner: str, pad: str = "40px 12px") -> str:
    return (
        '<!doctype html><html><head><meta charset="utf-8">'
        '<meta name="viewport" content="width=device-width,initial-scale=1"></head>'
        f'<body style="margin:0;padding:0;background:{bg}">'
        f'<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background:{bg}">'
        f'<tr><td align="center" style="padding:{pad}">{inner}</td></tr></table></body></html>'
    )


def card(style: str, rows: str, width: int = 600) -> str:
    return (
        f'<table role="presentation" width="{width}" cellpadding="0" cellspacing="0" '
        f'style="width:100%;max-width:{width}px;{style}">{rows}</table>'
    )


DESIGNS: list[tuple[str, str]] = []

# ---- Alltag -------------------------------------------------------------------

DESIGNS.append((
    "Klar",
    page("#ffffff", card("", (
        f'<tr><td style="padding:0 8px 28px;border-bottom:1px solid #ececf0;font-family:{SANS};'
        'font-size:12px;font-weight:600;letter-spacing:.08em;text-transform:uppercase;color:#8a8a90">{{name}}</td></tr>'
        f'<tr><td style="padding:32px 8px;font-family:{SANS};font-size:16px;line-height:1.7;color:#1d1d1f">{{{{inhalt}}}}</td></tr>'
        f'<tr><td style="padding:20px 8px 0;border-top:1px solid #ececf0;font-family:{SANS};font-size:12px;color:#8a8a90">'
        '<a href="mailto:{{email}}" style="color:#8a8a90;text-decoration:none">{{email}}</a></td></tr>'
    ), 560), "48px 20px"),
))

DESIGNS.append((
    "Papier",
    page("#f3efe8", card(
        "background:#fffdf8;border-radius:4px;border:1px solid #e7e0d3",
        (
            '<tr><td style="height:6px;background:#2b2620;font-size:0;line-height:0">&nbsp;</td></tr>'
            f'<tr><td style="padding:44px 52px 12px;font-family:{SERIF};font-size:13px;font-style:italic;color:#8b7f6e">'
            "{{name}}</td></tr>"
            f'<tr><td style="padding:8px 52px 44px;font-family:{SERIF};font-size:17px;line-height:1.75;color:#2b2620">{{{{inhalt}}}}</td></tr>'
        ),
        580,
    )),
))

DESIGNS.append((
    "Notiz",
    page("#f4f4f6", card(
        "background:#fff6c9;border-radius:6px",
        (
            '<tr><td style="padding:30px 36px 6px">'
            '<table role="presentation" cellpadding="0" cellspacing="0"><tr>'
            '<td style="width:10px;height:10px;border-radius:5px;background:#f2b705;font-size:0">&nbsp;</td>'
            f'<td style="padding-left:10px;font-family:{SANS};font-size:12px;font-weight:700;letter-spacing:.06em;'
            'text-transform:uppercase;color:#8a6d00">Kurz notiert</td></tr></table></td></tr>'
            f'<tr><td style="padding:10px 36px 0;font-family:{SERIF};font-size:24px;line-height:1.3;color:#3d3200">{{{{betreff}}}}</td></tr>'
            f'<tr><td style="padding:14px 36px 34px;font-family:{SANS};font-size:15px;line-height:1.65;color:#3d3a2a">{{{{inhalt}}}}</td></tr>'
        ),
        540,
    )),
))

# ---- Geschäftlich --------------------------------------------------------------

DESIGNS.append((
    "Geschäftsbrief",
    page("#eef1f5", card(
        "background:#ffffff;border-radius:10px;overflow:hidden",
        (
            '<tr><td style="padding:28px 44px;border-bottom:1px solid #e6eaf0">'
            '<table role="presentation" width="100%" cellpadding="0" cellspacing="0"><tr>'
            f'<td style="font-family:{SANS};font-size:18px;font-weight:700;letter-spacing:-.01em;color:#1f3a5f">{{{{name}}}}</td>'
            f'<td align="right" style="font-family:{SANS};font-size:12px;color:#7a8699">'
            '<a href="mailto:{{email}}" style="color:#7a8699;text-decoration:none">{{email}}</a></td>'
            "</tr></table></td></tr>"
            f'<tr><td style="padding:36px 44px 8px;font-family:{SANS};font-size:13px;font-weight:600;color:#1f3a5f">'
            "Betreff: {{betreff}}</td></tr>"
            f'<tr><td style="padding:12px 44px 40px;font-family:{SANS};font-size:15px;line-height:1.7;color:#25303f">{{{{inhalt}}}}</td></tr>'
            f'<tr><td style="padding:20px 44px;background:#1f3a5f;font-family:{SANS};font-size:12px;color:#c9d6e8">'
            '{{name}} &nbsp;·&nbsp; <a href="mailto:{{email}}" style="color:#ffffff;text-decoration:none">{{email}}</a></td></tr>'
        ),
    )),
))

DESIGNS.append((
    "Angebot",
    page("#f1f3f6", card(
        "background:#ffffff;border-radius:16px;overflow:hidden",
        (
            '<tr><td style="padding:44px 44px 40px;background:#0f172a">'
            f'<div style="font-family:{SANS};font-size:12px;font-weight:700;letter-spacing:.12em;text-transform:uppercase;color:#7dd3fc">{{{{name}}}}</div>'
            f'<div style="margin-top:14px;font-family:{SANS};font-size:30px;line-height:1.2;font-weight:800;letter-spacing:-.02em;color:#ffffff">{{{{betreff}}}}</div>'
            '<div style="margin-top:20px;width:48px;height:4px;border-radius:2px;background:#38bdf8;font-size:0;line-height:0">&nbsp;</div>'
            "</td></tr>"
            f'<tr><td style="padding:40px 44px 16px;font-family:{SANS};font-size:15px;line-height:1.7;color:#1e293b">{{{{inhalt}}}}</td></tr>'
            '<tr><td style="padding:8px 44px 40px">'
            '<table role="presentation" cellpadding="0" cellspacing="0"><tr>'
            f'<td style="border-radius:10px;background:#0f172a"><a href="mailto:{{{{email}}}}" style="display:inline-block;padding:13px 22px;font-family:{SANS};font-size:14px;font-weight:700;color:#ffffff;text-decoration:none">Antworten &rarr;</a></td>'
            "</tr></table></td></tr>"
            f'<tr><td style="padding:22px 44px;background:#f8fafc;border-top:1px solid #e2e8f0;font-family:{SANS};font-size:12px;color:#64748b">'
            "{{name}} · {{email}}</td></tr>"
        ),
    )),
))

DESIGNS.append((
    "Executive",
    page("#151517", card(
        "background:#ffffff",
        (
            '<tr><td style="height:3px;background:#b08d57;font-size:0;line-height:0">&nbsp;</td></tr>'
            f'<tr><td align="center" style="padding:44px 48px 8px;font-family:{SERIF};font-size:13px;letter-spacing:.22em;text-transform:uppercase;color:#b08d57">{{{{name}}}}</td></tr>'
            f'<tr><td align="center" style="padding:8px 48px 0;font-family:{SERIF};font-size:28px;line-height:1.25;color:#151517">{{{{betreff}}}}</td></tr>'
            '<tr><td align="center" style="padding:22px 0 6px"><div style="width:36px;height:1px;background:#b08d57;font-size:0;line-height:0">&nbsp;</div></td></tr>'
            f'<tr><td style="padding:24px 56px 48px;font-family:{SANS};font-size:15px;line-height:1.75;color:#2a2a2e">{{{{inhalt}}}}</td></tr>'
            f'<tr><td align="center" style="padding:22px 48px;background:#151517;font-family:{SERIF};font-size:12px;letter-spacing:.08em;color:#b08d57">'
            "{{email}}</td></tr>"
        ),
    ), "48px 12px"),
))

# ---- Privat & Anlässe ----------------------------------------------------------

DESIGNS.append((
    "Einladung",
    page("#fff1ea", card(
        "background:#ffffff;border-radius:22px;overflow:hidden",
        (
            '<tr><td align="center" style="padding:52px 40px 44px;background:#ff7a59">'
            f'<div style="font-family:{SERIF};font-size:18px;letter-spacing:.5em;color:#ffe3d8">&#10022; &#10022; &#10022;</div>'
            f'<div style="margin-top:14px;font-family:{SANS};font-size:12px;font-weight:700;letter-spacing:.24em;text-transform:uppercase;color:#ffffff">Einladung</div>'
            f'<div style="margin-top:12px;font-family:{SERIF};font-size:36px;line-height:1.15;color:#ffffff">{{{{betreff}}}}</div>'
            "</td></tr>"
            f'<tr><td align="center" style="padding:40px 48px 12px;font-family:{SANS};font-size:16px;line-height:1.75;color:#3b2a24">{{{{inhalt}}}}</td></tr>'
            f'<tr><td align="center" style="padding:12px 48px 48px;font-family:{SERIF};font-size:18px;font-style:italic;color:#ff7a59">Herzlich, {{{{name}}}}</td></tr>'
        ),
        560,
    )),
))

DOTS = "".join(
    f'<td style="padding:0 5px"><div style="width:{s}px;height:{s}px;border-radius:{s // 2}px;background:{c};font-size:0;line-height:0">&nbsp;</div></td>'
    for c, s in [
        ("#ff5d8f", 12), ("#ffc145", 8), ("#4cc9f0", 14), ("#7b61ff", 9),
        ("#3ddc97", 12), ("#ff8c42", 8), ("#ff5d8f", 10), ("#4cc9f0", 8),
    ]
)

DESIGNS.append((
    "Feier",
    page("#f6f2ff", card(
        "background:#ffffff;border-radius:22px",
        (
            f'<tr><td align="center" style="padding:40px 24px 0"><table role="presentation" cellpadding="0" cellspacing="0"><tr>{DOTS}</tr></table></td></tr>'
            f'<tr><td align="center" style="padding:26px 44px 0;font-family:{SANS};font-size:38px;line-height:1.1;font-weight:800;letter-spacing:-.02em;color:#7b61ff">{{{{betreff}}}}</td></tr>'
            f'<tr><td align="center" style="padding:24px 48px 16px;font-family:{SANS};font-size:16px;line-height:1.7;color:#2e2a3d">{{{{inhalt}}}}</td></tr>'
            f'<tr><td align="center" style="padding:8px 24px 40px"><table role="presentation" cellpadding="0" cellspacing="0"><tr>{DOTS}</tr></table>'
            f'<div style="margin-top:18px;font-family:{SANS};font-size:13px;font-weight:600;color:#8d86a3">{{{{name}}}}</div></td></tr>'
        ),
        560,
    )),
))

DESIGNS.append((
    "Sommer",
    page("#e9f7f4", card(
        "background:#ffffff;border-radius:18px;overflow:hidden",
        (
            '<tr><td style="padding:0;font-size:0;line-height:0">'
            '<table role="presentation" width="100%" cellpadding="0" cellspacing="0"><tr>'
            '<td style="height:10px;background:#0f766e">&nbsp;</td><td style="height:10px;background:#14b8a6">&nbsp;</td>'
            '<td style="height:10px;background:#fbbf24">&nbsp;</td><td style="height:10px;background:#fb7185">&nbsp;</td>'
            "</tr></table></td></tr>"
            f'<tr><td style="padding:40px 44px 0;font-family:{SANS};font-size:13px;font-weight:700;color:#0f766e">Hallo aus dem Postfach von {{{{name}}}} &#9728;</td></tr>'
            f'<tr><td style="padding:10px 44px 0;font-family:{SANS};font-size:30px;line-height:1.2;font-weight:800;letter-spacing:-.02em;color:#0b3b37">{{{{betreff}}}}</td></tr>'
            f'<tr><td style="padding:22px 44px 44px;font-family:{SANS};font-size:16px;line-height:1.7;color:#24423f">{{{{inhalt}}}}</td></tr>'
        ),
        580,
    )),
))


def sql_str(s: str) -> str:
    return "'" + s.replace("'", "''") + "'"


def main() -> None:
    out = [
        "-- 1.3: neue mitgelieferte Designs (erzeugt von scripts/gen-templates.py).",
        "-- Eigene Vorlagen bleiben unberührt.",
        "DELETE FROM templates WHERE builtin = 1;",
        "INSERT INTO templates (name, builtin, sort_order, html) VALUES",
    ]
    rows = [f"({sql_str(n)}, 1, {i}, {sql_str(h)})" for i, (n, h) in enumerate(DESIGNS)]
    out.append(",\n".join(rows) + ";")
    target = Path(__file__).resolve().parent.parent / "crates/store/migrations/011_designs.sql"
    target.write_text("\n".join(out) + "\n", encoding="utf-8")
    print(f"{len(DESIGNS)} Designs → {target}")


if __name__ == "__main__":
    main()
