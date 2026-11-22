-- 1.3.1: Erkennung brach beim ersten abgelehnten Server ab (imap.purelymail.com),
-- statt den nächsten (purelymail.com) zu fragen – erfolglose Suchen erneut starten.
UPDATE accounts SET carddav_home = NULL, carddav_checked = NULL WHERE carddav_home = '';
UPDATE accounts SET caldav_home = NULL, caldav_checked = NULL WHERE caldav_home = '';
