#!/usr/bin/env python3
"""Spielt Test-Mails in den lokalen Stalwart-Testserver ein (über IMAP APPEND).

Nutzung:
    python3 scripts/seed-mail.py            # ~40 abwechslungsreiche Mails
    python3 scripts/seed-mail.py --bulk 5000 # zusätzlich viele einfache Mails (Lasttest)

Nur für den lokalen Testserver (scripts/test-server.sh), nie für echte Postfächer.
"""

import argparse
import base64
import imaplib
import re
import random
from datetime import datetime, timedelta, timezone
from email.message import EmailMessage
from email.utils import format_datetime, make_msgid

HOST = "127.0.0.1"
IMAP_PORT = 1143
USER = "test@kolibri.test"
PASSWORD = "kolibri-test"
NOW = datetime.now(timezone.utc)

# 1×1-PNG (transparent) für eingebettete Bilder
PIXEL = base64.b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=="
)


def connect():
    imap = imaplib.IMAP4(HOST, IMAP_PORT)
    imap.login(USER, PASSWORD)
    return imap


def special_folders(imap):
    """Rolle (\\Sent usw.) → Ordnername, über LIST-Attribute (RFC 6154)."""
    roles = {"inbox": "INBOX"}
    _, lines = imap.list()
    for line in lines:
        text = line.decode()
        m = re.match(r'\((?P<flags>[^)]*)\) "(?P<delim>[^"]*)" "?(?P<name>[^"]*)"?', text)
        if not m:
            continue
        for flag in m.group("flags").split():
            role = flag.lstrip("\\").lower()
            if role in ("sent", "drafts", "trash", "junk", "archive"):
                roles[role] = m.group("name")
        roles.setdefault("_delim", m.group("delim"))
    return roles


def ensure_folder(imap, name):
    imap.create(f'"{name}"')  # existiert er schon, antwortet der Server mit NO – egal
    return name


def msg(subject, sender, body, date, to=USER, html=None, reply_to=None, attach=None, cid=False):
    m = EmailMessage()
    m["From"] = sender
    m["To"] = to
    m["Subject"] = subject
    m["Date"] = format_datetime(date)
    m["Message-ID"] = make_msgid(domain="kolibri.test")
    if reply_to:
        m["In-Reply-To"] = reply_to
        m["References"] = reply_to
    m.set_content(body)
    if html:
        m.add_alternative(html, subtype="html")
        if cid:
            html_part = m.get_payload()[1]
            html_part.add_related(PIXEL, "image", "png", cid="<logo@kolibri.test>")
    if attach:
        name, data, maintype, subtype = attach
        m.add_attachment(data, maintype=maintype, subtype=subtype, filename=name)
    return m


def scenario():
    """Abwechslungsreiche Mails – deckt typische Fälle ab."""
    d = lambda days, hours=0: NOW - timedelta(days=days, hours=hours)  # noqa: E731
    out = []

    first = msg(
        "Projekt Kolibri – Kick-off am Montag",
        "Anna Schäfer <anna@beispiel.de>",
        "Hallo,\n\nlass uns am Montag um 10 Uhr starten. Agenda folgt.\n\nViele Grüße\nAnna",
        d(3),
    )
    out.append((first, {"$seen": True}, "inbox"))
    reply = msg(
        "Re: Projekt Kolibri – Kick-off am Montag",
        "Test <test@kolibri.test>",
        "Passt mir gut, bis Montag!\n\n> lass uns am Montag um 10 Uhr starten.",
        d(2, 20),
        to="anna@beispiel.de",
        reply_to=first["Message-ID"],
    )
    out.append((reply, {"$seen": True}, "sent"))
    out.append(
        (
            msg(
                "Re: Projekt Kolibri – Kick-off am Montag",
                "Anna Schäfer <anna@beispiel.de>",
                "Super, Agenda im Anhang.",
                d(2, 2),
                reply_to=reply["Message-ID"],
                attach=("Agenda.pdf", b"%PDF-1.4\n% Test\n", "application", "pdf"),
            ),
            {},
            "inbox",
        )
    )

    newsletter_html = """<html><body style="margin:0;font-family:Helvetica,Arial,sans-serif">
<table width="100%" bgcolor="#f4f4f4" cellpadding="0" cellspacing="0"><tr><td align="center">
<table width="600" bgcolor="#ffffff" cellpadding="24"><tr><td>
<img src="https://example.com/banner.jpg" width="552" alt="Banner">
<h1 style="color:#222">Herbst-Angebote 🍂</h1>
<p style="color:#555;line-height:1.5">Bis zu <b>30 %</b> auf ausgewählte Artikel.
<a href="https://example.com/angebote?utm_source=mail">Jetzt ansehen</a></p>
<script>alert('böse')</script>
<p onclick="alert(1)" style="font-size:12px;color:#999">Abmelden: <a href="https://example.com/abmelden">hier</a></p>
<img src="https://tracker.example.com/open.gif?id=123" width="1" height="1">
</td></tr></table></td></tr></table></body></html>"""
    out.append(
        (
            msg("Herbst-Angebote: bis zu 30 % Rabatt", "Shop <news@shop.example>", "Herbst-Angebote", d(1), html=newsletter_html),
            {},
            "inbox",
        )
    )

    cid_html = '<p>Hallo! Unser Logo ist eingebettet:</p><p><img src="cid:logo@kolibri.test" width="40" height="40"></p><p>Grüße vom Kolibri-Team</p>'
    out.append(
        (
            msg("Eingebettetes Bild (cid)", "Kolibri-Team <team@kolibri.test>", "Logo eingebettet", d(0, 5), html=cid_html, cid=True),
            {"$flagged": True},
            "inbox",
        )
    )

    out.append(
        (
            msg("Rechnung Oktober", "Stadtwerke <rechnung@stadtwerke.example>",
                "Ihre Rechnung für Oktober liegt bei.", d(20),
                attach=("Rechnung-2026-10.pdf", b"%PDF-1.4\n% Rechnung\n", "application", "pdf")),
            {"$seen": True},
            "inbox",
        )
    )
    out.append(
        (
            msg("Ümläute & Sönderzeichen im Betreff – ß, €, 日本語", "Jürgen Müller <juergen@beispiel.de>",
                "Grüße aus Köln! Größe: 5 m², Preis: 12,50 €", d(5)),
            {},
            "inbox",
        )
    )
    out.append(
        (
            msg("Sehr alte Mail (vor 8 Monaten)", "Archiv <alt@beispiel.de>",
                "Diese Mail ist älter als 6 Monate und wird nur als Kopf geladen.", d(240)),
            {"$seen": True},
            "inbox",
        )
    )
    out.append(
        (
            msg("Notiz für Kolibri-Ordner", "Anna Schäfer <anna@beispiel.de>", "Liegt im Unterordner.", d(7)),
            {"$seen": True},
            "Kolibri",
        )
    )

    senders = [
        "Lena Wolf <lena@beispiel.de>", "Max Braun <max@firma.example>",
        "GitHub <noreply@github.example>", "Bahn <service@bahn.example>",
        "Paul Neumann <paul@beispiel.de>", "Sophie Klein <sophie@beispiel.de>",
    ]
    topics = [
        ("Treffen nächste Woche?", "Hast du Dienstag oder Mittwoch Zeit?"),
        ("Fotos vom Wochenende", "Hier ein paar Eindrücke vom Ausflug."),
        ("[kolibri] Neuer Pull Request", "Bitte einmal drüberschauen."),
        ("Ihre Reiseverbindung", "Abfahrt 08:12, Gleis 4."),
        ("Kurze Frage", "Weißt du noch, wie das Restaurant hieß?"),
        ("Geburtstagseinladung 🎉", "Wir feiern am Samstag ab 19 Uhr."),
    ]
    rnd = random.Random(42)
    for i in range(30):
        subj, body = topics[i % len(topics)]
        days = rnd.randint(0, 170)
        keywords = {"$seen": True} if rnd.random() < 0.6 else {}
        if rnd.random() < 0.1:
            keywords["$flagged"] = True
        out.append((msg(subj, senders[i % len(senders)], body + f"\n\n(Nr. {i + 1})", d(days, rnd.randint(0, 23))), keywords, "inbox"))
    return out


def bulk(n):
    rnd = random.Random(7)
    for i in range(n):
        date = NOW - timedelta(minutes=rnd.randint(0, 60 * 24 * 175))
        yield (
            msg(f"Lasttest-Mail #{i + 1}", f"Absender {i % 97} <a{i % 97}@last.example>",
                "Inhalt " * rnd.randint(5, 60), date),
            {"$seen": True} if rnd.random() < 0.8 else {},
            "inbox",
        )


CONTACTS = [
    ("anna", "Anna Schäfer", "Schäfer;Anna", ["anna@beispiel.de", "anna.schaefer@firma.example"], "+49 170 1234567", "Beispiel GmbH"),
    ("max", "Max Braun", "Braun;Max", ["max@firma.example"], "+49 221 987654", "Firma AG"),
    ("juergen", "Jürgen Müller", "Müller;Jürgen", ["juergen@beispiel.de"], "", ""),
    ("lena", "Lena Wolf", "Wolf;Lena", ["lena@beispiel.de"], "+49 151 555000", ""),
]


def seed_contacts():
    """Legt Test-Kontakte per CardDAV (HTTP PUT) im Standard-Adressbuch an."""
    import base64
    import urllib.request

    auth = "Basic " + base64.b64encode(f"{USER}:{PASSWORD}".encode()).decode()
    base = "http://127.0.0.1:18080/dav/card/test%40kolibri.test/default/"
    for uid, fn, n, mails, tel, org in CONTACTS:
        lines = ["BEGIN:VCARD", "VERSION:3.0", f"UID:{uid}", f"FN:{fn}", f"N:{n};;;"]
        lines += [f"EMAIL;TYPE=INTERNET{',WORK' if i else ''}:{m}" for i, m in enumerate(mails)]
        if tel:
            lines.append(f"TEL;TYPE=CELL:{tel}")
        if org:
            lines.append(f"ORG:{org}")
        lines.append("END:VCARD")
        req = urllib.request.Request(base + uid + ".vcf", data="\r\n".join(lines).encode(), method="PUT")
        req.add_header("Authorization", auth)
        req.add_header("Content-Type", "text/vcard; charset=utf-8")
        urllib.request.urlopen(req, timeout=30).read()
    print(f"{len(CONTACTS)} Kontakte angelegt.")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--bulk", type=int, default=0, help="zusätzliche einfache Mails")
    parser.add_argument("--no-scenario", action="store_true")
    parser.add_argument("--no-contacts", action="store_true")
    args = parser.parse_args()

    imap = connect()
    boxes = special_folders(imap)
    delim = boxes.get("_delim", "/")
    ensure_folder(imap, "Projekte")
    boxes["Kolibri"] = ensure_folder(imap, f"Projekte{delim}Kolibri")

    items = [] if args.no_scenario else scenario()
    items += list(bulk(args.bulk))

    flag_map = {"$seen": "\\Seen", "$flagged": "\\Flagged"}
    imported = 0
    for m, keywords, box in items:
        flags = " ".join(flag_map[k] for k in keywords if k in flag_map)
        date = imaplib.Time2Internaldate(m["Date"].datetime)
        status, _ = imap.append(f'"{boxes[box]}"', f"({flags})" if flags else None, date, bytes(m))
        if status == "OK":
            imported += 1
        else:
            print("Fehler beim Einspielen:", m["Subject"])
    imap.logout()
    print(f"{imported} Mails eingespielt.")
    if not args.no_contacts and not args.no_scenario:
        seed_contacts()


if __name__ == "__main__":
    main()
