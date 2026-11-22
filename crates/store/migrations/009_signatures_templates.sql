-- 1.2: Mehrere HTML-Signaturen je Konto, HTML-Vorlagen, Zusatzdaten für Entwürfe.

CREATE TABLE signatures (
    id          INTEGER PRIMARY KEY,
    account_id  INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    name        TEXT    NOT NULL,
    html        TEXT    NOT NULL DEFAULT '',   -- gesäubertes HTML-Fragment
    is_default  INTEGER NOT NULL DEFAULT 0,    -- wird beim Verfassen eingesetzt
    sort_order  INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE INDEX signatures_account ON signatures(account_id, sort_order, id);

-- Bisherige Klartext-Signatur übernehmen (als HTML, Zeilenumbrüche → <br>).
INSERT INTO signatures (account_id, name, html, is_default)
SELECT id, 'Standard',
       '<p>' || replace(replace(replace(replace(trim(signature, ' ' || char(9) || char(10) || char(13)), '&', '&amp;'), '<', '&lt;'),
                                '>', '&gt;'), char(10), '<br>') || '</p>',
       1
FROM accounts WHERE trim(signature, ' ' || char(9) || char(10) || char(13)) <> '';

-- Vorlagen: vollständige HTML-Seiten. {{inhalt}} wird durch den geschriebenen Text
-- ersetzt; ohne {{inhalt}} ist die Vorlage eine fertige Mail (nur Code).
-- Weitere Platzhalter: {{name}}, {{email}}, {{betreff}}.
CREATE TABLE templates (
    id          INTEGER PRIMARY KEY,
    name        TEXT    NOT NULL,
    html        TEXT    NOT NULL,
    builtin     INTEGER NOT NULL DEFAULT 0,
    sort_order  INTEGER NOT NULL DEFAULT 0
) STRICT;

INSERT INTO templates (name, builtin, sort_order, html) VALUES
('Schlicht', 1, 0,
'<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"></head>
<body style="margin:0;padding:0;background:#f4f4f6">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background:#f4f4f6"><tr><td align="center" style="padding:32px 12px">
<table role="presentation" width="600" cellpadding="0" cellspacing="0" style="width:100%;max-width:600px;background:#ffffff;border-radius:14px">
<tr><td style="padding:36px 40px;font-family:-apple-system,Helvetica,Arial,sans-serif;font-size:15px;line-height:1.6;color:#1d1d1f">{{inhalt}}</td></tr>
</table></td></tr></table></body></html>'),
('Akzent', 1, 1,
'<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"></head>
<body style="margin:0;padding:0;background:#efeff2">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background:#efeff2"><tr><td align="center" style="padding:32px 12px">
<table role="presentation" width="600" cellpadding="0" cellspacing="0" style="width:100%;max-width:600px;background:#ffffff;border-radius:14px;overflow:hidden">
<tr><td style="background:#111113;padding:28px 40px;font-family:-apple-system,Helvetica,Arial,sans-serif;font-size:20px;font-weight:700;letter-spacing:-0.01em;color:#ffffff">{{name}}</td></tr>
<tr><td style="padding:36px 40px 28px;font-family:-apple-system,Helvetica,Arial,sans-serif;font-size:15px;line-height:1.6;color:#1d1d1f">{{inhalt}}</td></tr>
<tr><td style="padding:18px 40px 26px;border-top:1px solid #ececf0;font-family:-apple-system,Helvetica,Arial,sans-serif;font-size:12px;color:#8a8a90">{{name}} · <a href="mailto:{{email}}" style="color:#8a8a90">{{email}}</a></td></tr>
</table></td></tr></table></body></html>'),
('Brief', 1, 2,
'<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"></head>
<body style="margin:0;padding:0;background:#f7f3ec">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background:#f7f3ec"><tr><td align="center" style="padding:40px 12px">
<table role="presentation" width="600" cellpadding="0" cellspacing="0" style="width:100%;max-width:600px;background:#fffdf9;border-top:3px solid #2b2620">
<tr><td style="padding:40px 48px;font-family:Georgia,serif;font-size:16px;line-height:1.7;color:#2b2620">{{inhalt}}</td></tr>
</table></td></tr></table></body></html>'),
('Ankündigung', 1, 3,
'<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"></head>
<body style="margin:0;padding:0;background:#f4f4f6">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background:#f4f4f6"><tr><td align="center" style="padding:32px 12px">
<table role="presentation" width="600" cellpadding="0" cellspacing="0" style="width:100%;max-width:600px;background:#ffffff;border-radius:14px;overflow:hidden">
<tr><td style="height:6px;background:#d9542b;font-size:0;line-height:0">&nbsp;</td></tr>
<tr><td style="padding:36px 40px 8px;font-family:-apple-system,Helvetica,Arial,sans-serif;font-size:26px;line-height:1.25;font-weight:700;letter-spacing:-0.02em;color:#111113">{{betreff}}</td></tr>
<tr><td style="padding:12px 40px 36px;font-family:-apple-system,Helvetica,Arial,sans-serif;font-size:15px;line-height:1.6;color:#3a3a3f">{{inhalt}}</td></tr>
<tr><td style="padding:20px 40px;background:#fafafb;font-family:-apple-system,Helvetica,Arial,sans-serif;font-size:12px;color:#8a8a90">{{name}} · {{email}}</td></tr>
</table></td></tr></table></body></html>');

-- Zusatzdaten eines Entwurfs, die nicht in der Mail selbst stehen (nur auf diesem Mac):
-- gewählte Vorlage/Signatur, Editor-Inhalt bzw. HTML-Code.
CREATE TABLE draft_meta (
    draft_id     TEXT    PRIMARY KEY,
    template_id  INTEGER,
    signature_id INTEGER,
    editor_html  TEXT    NOT NULL DEFAULT '',
    code         TEXT,
    updated_at   INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
