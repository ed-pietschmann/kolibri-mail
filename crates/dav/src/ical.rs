//! iCalendar (RFC 5545): Termine lesen, Wiederholungen aufklappen, Termine
//! und Einladungsantworten schreiben. Bewusst auf das beschränkt, was ein
//! Kalender für den Alltag braucht.
//!
//! Zeiten: „Z“ = UTC, `TZID=…` = benannte Zone (IANA, gängige Windows-Namen
//! werden übersetzt), sonst „schwebend“ = Zeitzone des Macs. Ganztägige
//! Termine bleiben Kalendertage. Wiederholungen werden in der Zone des Termins
//! gerechnet – ein wöchentlicher Termin um 10 Uhr bleibt nach der
//! Zeitumstellung um 10 Uhr.

use jiff::Timestamp;
use jiff::civil::DateTime;
use jiff::tz::TimeZone;
use serde::Serialize;

// ---- Kalendertage als Zahl (Tage seit 1970-01-01) -----------------------------

pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 0 = Montag … 6 = Sonntag.
pub fn weekday(days: i64) -> i64 {
    (days + 3).rem_euclid(7)
}

fn days_in_month(y: i64, m: i64) -> i64 {
    let next = if m == 12 {
        days_from_civil(y + 1, 1, 1)
    } else {
        days_from_civil(y, m + 1, 1)
    };
    next - days_from_civil(y, m, 1)
}

// ---- Zeitzonen -------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Zone {
    Utc,
    /// Ohne Zone: gilt in der Zeitzone des Macs.
    Floating,
    Named(TimeZone),
}

/// Häufige Windows-/Outlook-Zonennamen → IANA.
fn windows_zone(name: &str) -> Option<&'static str> {
    Some(match name {
        "W. Europe Standard Time" => "Europe/Berlin",
        "Central Europe Standard Time" => "Europe/Budapest",
        "Central European Standard Time" => "Europe/Warsaw",
        "Romance Standard Time" => "Europe/Paris",
        "GMT Standard Time" => "Europe/London",
        "Greenwich Standard Time" => "Atlantic/Reykjavik",
        "E. Europe Standard Time" => "Europe/Chisinau",
        "FLE Standard Time" => "Europe/Kiev",
        "Eastern Standard Time" => "America/New_York",
        "Central Standard Time" => "America/Chicago",
        "Mountain Standard Time" => "America/Denver",
        "Pacific Standard Time" => "America/Los_Angeles",
        "Tokyo Standard Time" => "Asia/Tokyo",
        "China Standard Time" => "Asia/Shanghai",
        "India Standard Time" => "Asia/Kolkata",
        "AUS Eastern Standard Time" => "Australia/Sydney",
        "UTC" | "Coordinated Universal Time" => "UTC",
        _ => return None,
    })
}

pub fn zone_by_name(name: &str) -> Zone {
    let name = name.trim().trim_matches('"');
    // Manche Programme stellen einen Pfad voran („/mozilla.org/…/Europe/Berlin“).
    let candidates = [
        Some(name),
        windows_zone(name),
        name.rsplit_once("zoneinfo/").map(|(_, n)| n),
    ];
    for n in candidates.into_iter().flatten() {
        if n.eq_ignore_ascii_case("utc") || n.eq_ignore_ascii_case("etc/utc") {
            return Zone::Utc;
        }
        if let Ok(tz) = TimeZone::get(n) {
            return Zone::Named(tz);
        }
    }
    Zone::Floating
}

fn system_tz() -> TimeZone {
    TimeZone::system()
}

/// Ortszeit (Tag + Sekunde des Tages) in einer Zone → Unix-Sekunden.
pub fn to_unix(days: i64, secs: i64, zone: &Zone) -> i64 {
    let tz = match zone {
        Zone::Utc => return days * 86_400 + secs,
        Zone::Floating => system_tz(),
        Zone::Named(tz) => tz.clone(),
    };
    let (y, m, d) = civil_from_days(days);
    let (h, mi, s) = (secs / 3600, (secs / 60) % 60, secs % 60);
    let dt = DateTime::new(
        i16::try_from(y).unwrap_or(1970),
        i8::try_from(m).unwrap_or(1),
        i8::try_from(d).unwrap_or(1),
        i8::try_from(h).unwrap_or(0),
        i8::try_from(mi).unwrap_or(0),
        i8::try_from(s).unwrap_or(0),
        0,
    );
    match dt.and_then(|dt| dt.to_zoned_datetime(tz)) {
        Ok(z) => z.timestamp().as_second(),
        Err(_) => days * 86_400 + secs,
    }
}

/// Unix-Sekunden → (Tag, Sekunde des Tages) in der Zeitzone des Macs.
pub fn local_days(unix: i64) -> (i64, i64) {
    let Ok(ts) = Timestamp::from_second(unix) else {
        return (unix.div_euclid(86_400), unix.rem_euclid(86_400));
    };
    let dt = ts.to_zoned_datetime(system_tz()).datetime();
    let days = days_from_civil(
        i64::from(dt.year()),
        i64::from(dt.month()),
        i64::from(dt.day()),
    );
    let t = dt.time();
    (
        days,
        i64::from(t.hour()) * 3600 + i64::from(t.minute()) * 60 + i64::from(t.second()),
    )
}

// ---- Werte -----------------------------------------------------------------------

/// DATE oder DATE-TIME mit Zone.
#[derive(Debug, Clone, PartialEq)]
pub struct When {
    pub days: i64,
    /// None = ganzer Tag (VALUE=DATE).
    pub secs: Option<i64>,
    pub zone: Zone,
}

impl When {
    pub fn unix(&self) -> i64 {
        match self.secs {
            Some(s) => to_unix(self.days, s, &self.zone),
            // Ganztägig: Mitternacht in der Zeitzone des Macs.
            None => to_unix(self.days, 0, &Zone::Floating),
        }
    }
}

fn parse_when(value: &str, params: &[(String, String)]) -> Option<When> {
    let v = value.trim();
    let date = v.get(0..8)?;
    let y: i64 = date.get(0..4)?.parse().ok()?;
    let m: i64 = date.get(4..6)?.parse().ok()?;
    let d: i64 = date.get(6..8)?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let days = days_from_civil(y, m, d);
    let tzid = params
        .iter()
        .find(|(k, _)| k == "TZID")
        .map(|(_, v)| v.as_str());
    let Some(time) = v.get(9..15).filter(|_| v.as_bytes().get(8) == Some(&b'T')) else {
        return Some(When {
            days,
            secs: None,
            zone: Zone::Floating,
        });
    };
    let h: i64 = time.get(0..2)?.parse().ok()?;
    let mi: i64 = time.get(2..4)?.parse().ok()?;
    let s: i64 = time.get(4..6)?.parse().ok()?;
    let zone = if v.ends_with('Z') {
        Zone::Utc
    } else if let Some(t) = tzid {
        zone_by_name(t)
    } else {
        Zone::Floating
    };
    Some(When {
        days,
        secs: Some(h * 3600 + mi * 60 + s.min(59)),
        zone,
    })
}

/// „P1DT2H30M“, „-PT15M“, „P1W“ → Sekunden.
fn parse_duration(value: &str) -> Option<i64> {
    let v = value.trim();
    let (sign, v) = match v.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, v.strip_prefix('+').unwrap_or(v)),
    };
    let v = v.strip_prefix('P')?;
    let mut total = 0i64;
    let mut num = String::new();
    let mut in_time = false;
    for c in v.chars() {
        match c {
            '0'..='9' => num.push(c),
            'T' => in_time = true,
            'W' | 'D' | 'H' | 'M' | 'S' => {
                let n: i64 = num.parse().ok()?;
                num.clear();
                total += n * match (c, in_time) {
                    ('W', _) => 604_800,
                    ('D', _) => 86_400,
                    ('H', true) => 3600,
                    ('M', true) => 60,
                    ('S', true) => 1,
                    _ => return None,
                };
            }
            _ => return None,
        }
    }
    Some(sign * total)
}

// ---- Wiederholungsregel ---------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub freq: Freq,
    pub interval: i64,
    pub count: Option<u32>,
    pub until: Option<When>,
    /// (n-tes Vorkommen im Monat, 0 = jedes; Wochentag 0 = Montag).
    pub by_day: Vec<(i64, i64)>,
    pub by_month_day: Vec<i64>,
}

fn parse_weekday(s: &str) -> Option<i64> {
    Some(match s {
        "MO" => 0,
        "TU" => 1,
        "WE" => 2,
        "TH" => 3,
        "FR" => 4,
        "SA" => 5,
        "SU" => 6,
        _ => return None,
    })
}

fn parse_rule(value: &str) -> Option<Rule> {
    let mut rule = Rule {
        freq: Freq::Daily,
        interval: 1,
        count: None,
        until: None,
        by_day: vec![],
        by_month_day: vec![],
    };
    let mut freq = None;
    for part in value.split(';') {
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        match k.to_uppercase().as_str() {
            "FREQ" => {
                freq = Some(match v.to_uppercase().as_str() {
                    "DAILY" => Freq::Daily,
                    "WEEKLY" => Freq::Weekly,
                    "MONTHLY" => Freq::Monthly,
                    "YEARLY" => Freq::Yearly,
                    // Stündlich/minütlich gibt es im Alltag kaum – nicht unterstützt.
                    _ => return None,
                });
            }
            "INTERVAL" => rule.interval = v.parse().ok().filter(|n: &i64| *n > 0).unwrap_or(1),
            "COUNT" => rule.count = v.parse().ok(),
            "UNTIL" => rule.until = parse_when(v, &[]),
            "BYDAY" => {
                for d in v.split(',') {
                    let d = d.trim().to_uppercase();
                    let (num, day) = d.split_at(d.len().saturating_sub(2));
                    if let Some(wd) = parse_weekday(day) {
                        rule.by_day.push((num.parse().unwrap_or(0), wd));
                    }
                }
            }
            "BYMONTHDAY" => {
                rule.by_month_day = v.split(',').filter_map(|n| n.trim().parse().ok()).collect()
            }
            _ => {}
        }
    }
    rule.freq = freq?;
    Some(rule)
}

/// Kalendertage (in der Zone des Termins) der Vorkommen ab `start`, aufsteigend,
/// höchstens bis `last_day`.
fn rule_days(rule: &Rule, start: i64, last_day: i64) -> Vec<i64> {
    const MAX: usize = 20_000;
    let mut out = Vec::new();
    let (sy, sm, sd) = civil_from_days(start);
    let push_sorted = |mut days: Vec<i64>, out: &mut Vec<i64>| {
        days.sort_unstable();
        days.dedup();
        out.extend(days.into_iter().filter(|d| *d >= start && *d <= last_day));
    };
    let mut k = 0i64;
    while out.len() < MAX {
        let step = k * rule.interval;
        let candidates: Vec<i64> = match rule.freq {
            Freq::Daily => vec![start + step],
            Freq::Weekly => {
                let week_start = start - weekday(start) + step * 7;
                if week_start > last_day {
                    break;
                }
                if rule.by_day.is_empty() {
                    vec![week_start + weekday(start)]
                } else {
                    rule.by_day.iter().map(|(_, wd)| week_start + wd).collect()
                }
            }
            Freq::Monthly | Freq::Yearly => {
                let months = if rule.freq == Freq::Monthly {
                    step
                } else {
                    step * 12
                };
                let total = sy * 12 + (sm - 1) + months;
                let (y, m) = (total.div_euclid(12), total.rem_euclid(12) + 1);
                let first = days_from_civil(y, m, 1);
                if first > last_day {
                    break;
                }
                let dim = days_in_month(y, m);
                let mut days = Vec::new();
                if rule.freq == Freq::Monthly && !rule.by_day.is_empty() {
                    for (n, wd) in &rule.by_day {
                        let all: Vec<i64> = (0..dim)
                            .map(|i| first + i)
                            .filter(|d| weekday(*d) == *wd)
                            .collect();
                        match *n {
                            0 => days.extend(all),
                            n if n > 0 => {
                                days.extend(all.get(usize::try_from(n - 1).unwrap_or(99)).copied())
                            }
                            n => days.extend(
                                all.len()
                                    .checked_sub(usize::try_from(-n).unwrap_or(99))
                                    .and_then(|i| all.get(i).copied()),
                            ),
                        }
                    }
                } else if rule.freq == Freq::Monthly && !rule.by_month_day.is_empty() {
                    for d in &rule.by_month_day {
                        let day = if *d < 0 { dim + d + 1 } else { *d };
                        if (1..=dim).contains(&day) {
                            days.push(first + day - 1);
                        }
                    }
                } else if sd <= dim {
                    // Den 31. gibt es nicht in jedem Monat, den 29.2. nicht in jedem Jahr: überspringen.
                    days.push(first + sd - 1);
                }
                days
            }
        };
        if rule.freq == Freq::Daily && candidates.first().is_some_and(|d| *d > last_day) {
            break;
        }
        push_sorted(candidates, &mut out);
        k += 1;
        if k > 100_000 {
            break;
        }
    }
    out
}

// ---- Termine ----------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub email: String,
    pub name: String,
    /// NEEDS-ACTION, ACCEPTED, DECLINED, TENTATIVE
    pub status: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub uid: String,
    pub summary: String,
    pub location: String,
    pub description: String,
    pub start: When,
    pub end: Option<When>,
    pub duration: Option<i64>,
    pub rule: Option<Rule>,
    pub exdates: Vec<i64>,
    pub recurrence_id: Option<When>,
    pub status: String,
    pub sequence: u32,
    pub organizer: Option<Person>,
    pub attendees: Vec<Person>,
    /// Ursprüngliche Zeilen DTSTART/DTEND/DURATION (für Antworten unverändert übernehmen).
    pub raw_times: Vec<String>,
}

impl Event {
    pub fn all_day(&self) -> bool {
        self.start.secs.is_none()
    }

    /// Dauer in Sekunden (ganztägig ohne Ende: 1 Tag, sonst 0).
    fn length(&self) -> i64 {
        if let Some(end) = &self.end {
            return (end.unix() - self.start.unix()).max(0);
        }
        self.duration
            .unwrap_or(if self.all_day() { 86_400 } else { 0 })
    }
}

/// Ganze iCalendar-Datei.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Calendar {
    /// REQUEST, REPLY, CANCEL … (nur bei Einladungen gesetzt).
    pub method: Option<String>,
    pub events: Vec<Event>,
}

/// Ein konkretes Vorkommen im Kalender.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub uid: String,
    pub summary: String,
    pub location: String,
    pub start: i64,
    pub end: i64,
    pub all_day: bool,
    pub recurring: bool,
    pub cancelled: bool,
}

fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = line.strip_prefix([' ', '\t'])
            && let Some(last) = lines.last_mut()
        {
            last.push_str(rest);
        } else if !line.is_empty() {
            lines.push(line.to_owned());
        }
    }
    lines
}

/// Zeile: Name, Parameter, Wert.
type Line = (String, Vec<(String, String)>, String);

/// „NAME;P1=a;P2="b:c":wert“ → (NAME, [(P1,a),(P2,b:c)], wert)
fn split_line(line: &str) -> Option<Line> {
    let mut in_quotes = false;
    let mut colon = None;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            ':' if !in_quotes => {
                colon = Some(i);
                break;
            }
            _ => {}
        }
    }
    let colon = colon?;
    let (head, value) = (&line[..colon], &line[colon + 1..]);
    let mut parts = head.split(';');
    let name = parts.next()?.to_uppercase();
    let params = parts
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.to_uppercase(), v.trim_matches('"').to_owned()))
        .collect();
    Some((name, params, value.to_owned()))
}

fn unescape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut chars = v.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n' | 'N') => out.push('\n'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn person(params: &[(String, String)], value: &str) -> Person {
    let get = |k: &str| {
        params
            .iter()
            .find(|(p, _)| p == k)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    Person {
        email: value
            .trim()
            .trim_start_matches("mailto:")
            .trim_start_matches("MAILTO:")
            .to_owned(),
        name: get("CN"),
        status: get("PARTSTAT"),
    }
}

pub fn parse(text: &str) -> Calendar {
    let mut cal = Calendar::default();
    let mut current: Option<Event> = None;
    let mut depth_other = 0; // in VALARM o. Ä. innerhalb eines VEVENT
    for line in unfold(text) {
        let Some((name, params, value)) = split_line(&line) else {
            continue;
        };
        match (name.as_str(), value.to_uppercase().as_str()) {
            ("BEGIN", "VEVENT") => {
                current = Some(Event {
                    uid: String::new(),
                    summary: String::new(),
                    location: String::new(),
                    description: String::new(),
                    start: When {
                        days: 0,
                        secs: None,
                        zone: Zone::Floating,
                    },
                    end: None,
                    duration: None,
                    rule: None,
                    exdates: vec![],
                    recurrence_id: None,
                    status: String::new(),
                    sequence: 0,
                    organizer: None,
                    attendees: vec![],
                    raw_times: vec![],
                });
                continue;
            }
            ("END", "VEVENT") => {
                if let Some(e) = current.take()
                    && e.start.days != 0
                {
                    cal.events.push(e);
                }
                continue;
            }
            ("BEGIN", _) if current.is_some() => {
                depth_other += 1;
                continue;
            }
            ("END", _) if depth_other > 0 => {
                depth_other -= 1;
                continue;
            }
            ("METHOD", _) if current.is_none() => {
                cal.method = Some(value.trim().to_uppercase());
                continue;
            }
            _ => {}
        }
        if depth_other > 0 {
            continue;
        }
        let Some(e) = current.as_mut() else { continue };
        match name.as_str() {
            "UID" => e.uid = value.trim().to_owned(),
            "SUMMARY" => e.summary = unescape(&value),
            "LOCATION" => e.location = unescape(&value),
            "DESCRIPTION" => e.description = unescape(&value),
            "STATUS" => e.status = value.trim().to_uppercase(),
            "SEQUENCE" => e.sequence = value.trim().parse().unwrap_or(0),
            "DTSTART" => {
                if let Some(w) = parse_when(&value, &params) {
                    e.start = w;
                }
                e.raw_times.push(line.clone());
            }
            "DTEND" => {
                e.end = parse_when(&value, &params);
                e.raw_times.push(line.clone());
            }
            "DURATION" => {
                e.duration = parse_duration(&value);
                e.raw_times.push(line.clone());
            }
            "RRULE" => e.rule = parse_rule(&value),
            "EXDATE" => {
                for v in value.split(',') {
                    if let Some(w) = parse_when(v, &params) {
                        e.exdates.push(w.unix());
                    }
                }
            }
            "RECURRENCE-ID" => e.recurrence_id = parse_when(&value, &params),
            "ORGANIZER" => e.organizer = Some(person(&params, &value)),
            "ATTENDEE" => e.attendees.push(person(&params, &value)),
            _ => {}
        }
    }
    cal
}

impl Calendar {
    /// Alle Vorkommen, die den Zeitraum [from, to) berühren, nach Beginn sortiert.
    pub fn instances(&self, from: i64, to: i64) -> Vec<Instance> {
        let mut out = Vec::new();
        for master in self.events.iter().filter(|e| e.recurrence_id.is_none()) {
            let overrides: Vec<&Event> = self
                .events
                .iter()
                .filter(|e| e.uid == master.uid && e.recurrence_id.is_some())
                .collect();
            let skip: Vec<i64> = overrides
                .iter()
                .filter_map(|o| o.recurrence_id.as_ref().map(When::unix))
                .chain(master.exdates.iter().copied())
                .collect();
            let length = master.length();
            let make = |start: i64, e: &Event, recurring: bool| Instance {
                uid: e.uid.clone(),
                summary: e.summary.clone(),
                location: e.location.clone(),
                start,
                end: start + e.length().max(if e.all_day() { 86_400 } else { 0 }),
                all_day: e.all_day(),
                recurring,
                cancelled: e.status == "CANCELLED",
            };
            match &master.rule {
                None => {
                    let s = master.start.unix();
                    if s < to && s + length.max(1) > from {
                        out.push(make(s, master, false));
                    }
                }
                Some(rule) => {
                    // Grobe Obergrenze in Tagen der Zone (+2 Tage Puffer für Zonenunterschiede).
                    let last_day = local_days(to).0 + 2;
                    let until = rule.until.as_ref().map(When::unix);
                    for (n, day) in rule_days(rule, master.start.days, last_day)
                        .into_iter()
                        .enumerate()
                    {
                        if rule.count.is_some_and(|c| n >= c as usize) {
                            break;
                        }
                        let start = match master.start.secs {
                            Some(s) => to_unix(day, s, &master.start.zone),
                            None => to_unix(day, 0, &Zone::Floating),
                        };
                        if until.is_some_and(|u| start > u) || start >= to {
                            break;
                        }
                        if start + length.max(1) > from && !skip.contains(&start) {
                            out.push(make(start, master, true));
                        }
                    }
                }
            }
            for o in overrides {
                let s = o.start.unix();
                if s < to && s + o.length().max(1) > from {
                    out.push(make(s, o, true));
                }
            }
        }
        out.sort_by_key(|i| (i.start, i.end));
        out
    }

    /// Letzter Zeitpunkt, an dem ein Vorkommen enden kann (None = unbegrenzt).
    /// Für die Datenbank: so findet eine Bereichsabfrage alle relevanten Termine.
    pub fn last_end(&self) -> Option<i64> {
        let mut last = i64::MIN;
        for e in &self.events {
            let base = e.start.unix() + e.length().max(86_400);
            match &e.rule {
                None => last = last.max(base),
                Some(r) => {
                    let end = if let Some(u) = &r.until {
                        u.unix() + e.length().max(86_400)
                    } else {
                        // Grob: höchstens c Jahre (jährlich ist das längste Intervall).
                        base + i64::from(r.count?) * r.interval * 366 * 86_400
                    };
                    last = last.max(end);
                }
            }
        }
        (last != i64::MIN).then_some(last)
    }
}

// ---- Schreiben ----------------------------------------------------------------------

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace("\r\n", "\\n")
        .replace('\n', "\\n")
}

/// Zeilen nach 75 Bytes umbrechen (RFC 5545, 3.1), ohne UTF-8 zu zerschneiden.
fn fold(line: &str) -> String {
    let mut out = String::new();
    let mut len = 0;
    for c in line.chars() {
        if len + c.len_utf8() > 75 {
            out.push_str("\r\n ");
            len = 1;
        }
        out.push(c);
        len += c.len_utf8();
    }
    out.push_str("\r\n");
    out
}

fn utc_stamp(unix: i64) -> String {
    let (y, m, d) = civil_from_days(unix.div_euclid(86_400));
    let s = unix.rem_euclid(86_400);
    format!(
        "{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z",
        s / 3600,
        (s / 60) % 60,
        s % 60
    )
}

fn date_stamp(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}{m:02}{d:02}")
}

/// Neuer oder geänderter Termin. Zeiten in UTC, ganztägig als Kalendertage
/// (Ende exklusiv, wie RFC 5545 es verlangt).
#[derive(Clone, Copy)]
pub struct NewEvent<'a> {
    pub uid: &'a str,
    pub summary: &'a str,
    pub location: &'a str,
    pub description: &'a str,
    pub all_day: bool,
    pub start: i64,
    pub end: i64,
    pub now: i64,
    /// Einladende Person (nur bei Terminen mit Teilnehmenden).
    pub organizer: Option<&'a Person>,
    pub attendees: &'a [Person],
    /// Zählt bei jeder Änderung hoch (iTIP).
    pub sequence: u32,
    /// REQUEST / CANCEL für Einladungs-Mails, sonst None.
    pub method: Option<&'a str>,
    /// Termin abgesagt (STATUS:CANCELLED).
    pub cancelled: bool,
}

/// `;CN="Name"` (leer, wenn kein Name).
fn cn_param(name: &str) -> String {
    let clean: String = name
        .chars()
        .filter(|c| !matches!(c, '"' | '\r' | '\n'))
        .collect();
    if clean.trim().is_empty() {
        String::new()
    } else {
        format!(";CN=\"{}\"", clean.trim())
    }
}

/// Teilnahme-Status, wie iCalendar ihn erwartet (unbekannt → NEEDS-ACTION).
fn partstat(status: &str) -> &'static str {
    match status.to_ascii_uppercase().as_str() {
        "ACCEPTED" => "ACCEPTED",
        "DECLINED" => "DECLINED",
        "TENTATIVE" => "TENTATIVE",
        "DELEGATED" => "DELEGATED",
        _ => "NEEDS-ACTION",
    }
}

pub fn write_event(e: &NewEvent<'_>) -> String {
    let mut lines = vec![
        "BEGIN:VCALENDAR".to_owned(),
        "VERSION:2.0".into(),
        "PRODID:-//Kolibri//Kolibri Mail//DE".into(),
    ];
    if let Some(m) = e.method {
        lines.push(format!("METHOD:{m}"));
    }
    lines.extend([
        "BEGIN:VEVENT".to_owned(),
        format!("UID:{}", e.uid),
        format!("DTSTAMP:{}", utc_stamp(e.now)),
    ]);
    if e.sequence > 0 {
        lines.push(format!("SEQUENCE:{}", e.sequence));
    }
    if e.all_day {
        let start = local_days(e.start).0;
        let end = local_days(e.end.max(e.start + 1) - 1).0 + 1;
        lines.push(format!("DTSTART;VALUE=DATE:{}", date_stamp(start)));
        lines.push(format!("DTEND;VALUE=DATE:{}", date_stamp(end)));
    } else {
        lines.push(format!("DTSTART:{}", utc_stamp(e.start)));
        lines.push(format!("DTEND:{}", utc_stamp(e.end.max(e.start))));
    }
    lines.push(format!("SUMMARY:{}", escape(e.summary)));
    if !e.location.is_empty() {
        lines.push(format!("LOCATION:{}", escape(e.location)));
    }
    if !e.description.is_empty() {
        lines.push(format!("DESCRIPTION:{}", escape(e.description)));
    }
    if e.cancelled {
        lines.push("STATUS:CANCELLED".into());
    }
    if let Some(o) = e.organizer {
        lines.push(format!("ORGANIZER{}:mailto:{}", cn_param(&o.name), o.email));
    }
    for a in e.attendees {
        lines.push(format!(
            "ATTENDEE{};ROLE=REQ-PARTICIPANT;PARTSTAT={};RSVP=TRUE:mailto:{}",
            cn_param(&a.name),
            partstat(&a.status),
            a.email
        ));
    }
    lines.push("END:VEVENT".into());
    lines.push("END:VCALENDAR".into());
    lines.iter().map(|l| fold(l)).collect()
}

/// Teilnahme-Status einer Person in einer gespeicherten iCalendar-Datei setzen
/// (Antwort auf eine eigene Einladung). `None`, wenn die Person nicht eingeladen ist.
pub fn set_attendee_status(ics: &str, email: &str, status: &str) -> Option<String> {
    let want = email.trim().to_ascii_lowercase();
    let mut found = false;
    let out: String = unfold(ics)
        .into_iter()
        .map(|line| {
            let Some((name, params, value)) = split_line(&line) else {
                return fold(&line);
            };
            let addr = value
                .trim()
                .trim_start_matches("mailto:")
                .trim_start_matches("MAILTO:");
            if name != "ATTENDEE" || !addr.eq_ignore_ascii_case(&want) {
                return fold(&line);
            }
            found = true;
            let mut out = String::from("ATTENDEE");
            for (k, v) in params.iter().filter(|(k, _)| k != "PARTSTAT") {
                if v.contains([';', ':', ',', ' ']) || k == "CN" {
                    out.push_str(&format!(";{k}=\"{}\"", v.replace('"', "")));
                } else {
                    out.push_str(&format!(";{k}={v}"));
                }
            }
            out.push_str(&format!(";PARTSTAT={}:{value}", partstat(status)));
            fold(&out)
        })
        .collect();
    found.then_some(out)
}

/// Antwort auf eine Einladung (iTIP REPLY, RFC 5546).
pub fn write_reply(event: &Event, me: &str, my_name: &str, accept: bool, now: i64) -> String {
    let status = if accept { "ACCEPTED" } else { "DECLINED" };
    let mut lines = vec![
        "BEGIN:VCALENDAR".to_owned(),
        "VERSION:2.0".into(),
        "PRODID:-//Kolibri//Kolibri Mail//DE".into(),
        "METHOD:REPLY".into(),
        "BEGIN:VEVENT".into(),
        format!("UID:{}", event.uid),
        format!("DTSTAMP:{}", utc_stamp(now)),
        format!("SEQUENCE:{}", event.sequence),
    ];
    lines.extend(event.raw_times.iter().cloned());
    if let Some(r) = &event.recurrence_id {
        lines.push(format!("RECURRENCE-ID:{}", utc_stamp(r.unix())));
    }
    if let Some(o) = &event.organizer {
        lines.push(format!("ORGANIZER:mailto:{}", o.email));
    }
    let cn = if my_name.is_empty() {
        String::new()
    } else {
        format!(";CN=\"{}\"", my_name.replace('"', ""))
    };
    lines.push(format!("ATTENDEE;PARTSTAT={status}{cn}:mailto:{me}"));
    lines.push(format!("SUMMARY:{}", escape(&event.summary)));
    lines.push("END:VEVENT".into());
    lines.push("END:VCALENDAR".into());
    lines.iter().map(|l| fold(l)).collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn writes_invitations_and_updates_replies() {
        let org = Person {
            email: "ich@x.de".into(),
            name: "Eddie".into(),
            status: String::new(),
        };
        let guests = vec![
            Person {
                email: "anna@y.de".into(),
                name: "Anna \"A\" Schäfer".into(),
                status: String::new(),
            },
            Person {
                email: "max@z.de".into(),
                name: String::new(),
                status: "accepted".into(),
            },
        ];
        let ics = write_event(&NewEvent {
            uid: "u1",
            summary: "Planung",
            location: "",
            description: "",
            all_day: false,
            start: 1_800_000_000,
            end: 1_800_003_600,
            now: 1_700_000_000,
            organizer: Some(&org),
            attendees: &guests,
            sequence: 2,
            method: Some("REQUEST"),
            cancelled: false,
        });
        assert!(ics.contains("METHOD:REQUEST\r\n"), "{ics}");
        assert!(ics.contains("SEQUENCE:2\r\n"));
        let cal = parse(&ics);
        let e = &cal.events[0];
        assert_eq!(e.organizer.as_ref().unwrap().email, "ich@x.de");
        assert_eq!(e.attendees.len(), 2);
        assert_eq!(e.attendees[0].name, "Anna A Schäfer");
        assert_eq!(e.attendees[0].status, "NEEDS-ACTION");
        assert_eq!(e.attendees[1].status, "ACCEPTED");
        assert_eq!(e.sequence, 2);

        let updated = set_attendee_status(&ics, "ANNA@y.de", "DECLINED").unwrap();
        let cal = parse(&updated);
        assert_eq!(cal.events[0].attendees[0].status, "DECLINED");
        assert_eq!(cal.events[0].attendees[1].status, "ACCEPTED");
        assert!(set_attendee_status(&ics, "fremd@q.de", "ACCEPTED").is_none());
    }

    fn berlin() -> Zone {
        zone_by_name("Europe/Berlin")
    }

    #[test]
    fn day_numbers() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(days_from_civil(2024, 2, 29)), (2024, 2, 29));
        assert_eq!(
            weekday(days_from_civil(2026, 11, 20)),
            4,
            "20.11.2026 ist ein Freitag"
        );
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2026, 2), 28);
    }

    #[test]
    fn parses_times_zones_and_durations() {
        let w = parse_when(
            "20260315T100000",
            &[("TZID".into(), "Europe/Berlin".into())],
        )
        .unwrap();
        assert_eq!(w.unix(), 1_773_565_200, "10:00 MEZ = 09:00 UTC");
        let w = parse_when(
            "20260715T100000",
            &[("TZID".into(), "W. Europe Standard Time".into())],
        )
        .unwrap();
        assert_eq!(
            w.unix(),
            1_784_102_400,
            "Outlook-Name, Sommerzeit: 08:00 UTC"
        );
        assert_eq!(
            parse_when("20260101T120000Z", &[]).unwrap().unix(),
            1_767_268_800
        );
        assert!(parse_when("20260101", &[]).unwrap().secs.is_none());
        assert_eq!(parse_duration("P1DT2H30M"), Some(95_400));
        assert_eq!(parse_duration("-PT15M"), Some(-900));
        assert_eq!(parse_duration("P2W"), Some(1_209_600));
    }

    const WEEKLY: &str = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:jour-fixe\r\nSUMMARY:Jour fixe\\, Team\r\n\
DTSTART;TZID=Europe/Berlin:20260316T100000\r\nDTEND;TZID=Europe/Berlin:20260316T110000\r\n\
RRULE:FREQ=WEEKLY;BYDAY=MO;COUNT=4\r\nEXDATE;TZID=Europe/Berlin:20260323T100000\r\n\
BEGIN:VALARM\r\nTRIGGER:-PT15M\r\nSUMMARY:Erinnerung\r\nEND:VALARM\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:jour-fixe\r\nRECURRENCE-ID;TZID=Europe/Berlin:20260330T100000\r\n\
SUMMARY:Jour fixe (verschoben)\r\nDTSTART;TZID=Europe/Berlin:20260331T140000\r\nDTEND;TZID=Europe/Berlin:20260331T150000\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn weekly_with_exdate_override_and_dst() {
        let cal = parse(WEEKLY);
        assert_eq!(cal.events.len(), 2);
        assert_eq!(
            cal.events[0].summary, "Jour fixe, Team",
            "VALARM-Summary darf nicht überschreiben"
        );
        let all = cal.instances(0, i64::MAX / 2);
        let titles: Vec<_> = all.iter().map(|i| i.summary.as_str()).collect();
        // 16.3., (23.3. gestrichen), 30.3. → verschoben auf 31.3., 6.4.
        assert_eq!(
            titles,
            [
                "Jour fixe, Team",
                "Jour fixe (verschoben)",
                "Jour fixe, Team"
            ]
        );
        // Nach der Zeitumstellung (29.3.) weiterhin 10:00 Ortszeit = 08:00 UTC.
        let april = all.last().unwrap();
        assert_eq!(
            april.start,
            to_unix(days_from_civil(2026, 4, 6), 10 * 3600, &berlin())
        );
        assert_eq!(april.start % 86_400, 8 * 3600);
        assert_eq!(
            all[0].start % 86_400,
            9 * 3600,
            "vor der Umstellung 09:00 UTC"
        );
        assert!(all.iter().all(|i| i.end - i.start == 3600 && i.recurring));
        // Bereichsabfrage.
        let march = cal.instances(
            to_unix(days_from_civil(2026, 3, 20), 0, &Zone::Utc),
            to_unix(days_from_civil(2026, 4, 1), 0, &Zone::Utc),
        );
        assert_eq!(march.len(), 1);
        assert!(cal.last_end().is_some());
    }

    #[test]
    fn monthly_and_yearly_rules() {
        let rule = |r: &str| parse_rule(r).unwrap();
        let start = days_from_civil(2026, 1, 31);
        let days = rule_days(&rule("FREQ=MONTHLY"), start, days_from_civil(2026, 6, 30));
        let dates: Vec<_> = days.iter().map(|d| civil_from_days(*d)).collect();
        assert_eq!(
            dates,
            [(2026, 1, 31), (2026, 3, 31), (2026, 5, 31)],
            "kein 31. im Feb/Apr/Jun"
        );

        let first_mon = days_from_civil(2026, 1, 12);
        let days = rule_days(
            &rule("FREQ=MONTHLY;BYDAY=2MO"),
            first_mon,
            days_from_civil(2026, 3, 31),
        );
        assert_eq!(
            days.iter().map(|d| civil_from_days(*d)).collect::<Vec<_>>(),
            [(2026, 1, 12), (2026, 2, 9), (2026, 3, 9)]
        );
        let days = rule_days(
            &rule("FREQ=MONTHLY;BYDAY=-1FR"),
            days_from_civil(2026, 1, 30),
            days_from_civil(2026, 3, 31),
        );
        assert_eq!(
            days.iter().map(|d| civil_from_days(*d)).collect::<Vec<_>>(),
            [(2026, 1, 30), (2026, 2, 27), (2026, 3, 27)]
        );

        let leap = days_from_civil(2024, 2, 29);
        let days = rule_days(&rule("FREQ=YEARLY"), leap, days_from_civil(2032, 12, 31));
        assert_eq!(
            days.iter()
                .map(|d| civil_from_days(*d).0)
                .collect::<Vec<_>>(),
            [2024, 2028, 2032]
        );

        let days = rule_days(
            &rule("FREQ=WEEKLY;INTERVAL=2;BYDAY=TU,TH"),
            days_from_civil(2026, 11, 3),
            days_from_civil(2026, 11, 30),
        );
        assert_eq!(
            days.iter()
                .map(|d| civil_from_days(*d).2)
                .collect::<Vec<_>>(),
            [3, 5, 17, 19]
        );
        assert!(parse_rule("FREQ=HOURLY").is_none());
    }

    #[test]
    fn all_day_and_until() {
        let cal = parse(
            "BEGIN:VEVENT\nUID:u\nSUMMARY:Urlaub\nDTSTART;VALUE=DATE:20260803\nDTEND;VALUE=DATE:20260808\nEND:VEVENT\n\
BEGIN:VEVENT\nUID:d\nSUMMARY:Täglich\nDTSTART:20260101T080000Z\nDURATION:PT30M\nRRULE:FREQ=DAILY;UNTIL=20260103T080000Z\nEND:VEVENT\n",
        );
        let all = cal.instances(0, i64::MAX / 2);
        let urlaub = all.iter().find(|i| i.summary == "Urlaub").unwrap();
        assert!(urlaub.all_day);
        assert_eq!((urlaub.end - urlaub.start) / 86_400, 5);
        assert_eq!(
            all.iter().filter(|i| i.summary == "Täglich").count(),
            3,
            "UNTIL ist inklusive"
        );
    }

    #[test]
    fn invitation_and_reply() {
        let invite = "BEGIN:VCALENDAR\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nUID:inv-1\r\nSEQUENCE:2\r\n\
SUMMARY:Planung\r\nDTSTART;TZID=Europe/Berlin:20261201T090000\r\nDTEND;TZID=Europe/Berlin:20261201T100000\r\n\
ORGANIZER;CN=\"Anna Schäfer\":mailto:anna@beispiel.de\r\nATTENDEE;CN=Ich;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:ich@x.de\r\n\
END:VEVENT\r\nEND:VCALENDAR\r\n";
        let cal = parse(invite);
        assert_eq!(cal.method.as_deref(), Some("REQUEST"));
        let e = &cal.events[0];
        assert_eq!(e.organizer.as_ref().unwrap().name, "Anna Schäfer");
        assert_eq!(e.attendees[0].status, "NEEDS-ACTION");
        let reply = write_reply(e, "ich@x.de", "Ich", true, 0);
        assert!(reply.contains("METHOD:REPLY\r\n"));
        assert!(reply.contains("ATTENDEE;PARTSTAT=ACCEPTED;CN=\"Ich\":mailto:ich@x.de"));
        assert!(reply.contains("DTSTART;TZID=Europe/Berlin:20261201T090000"));
        assert!(reply.contains("SEQUENCE:2"));
        let back = parse(&reply);
        assert_eq!(back.events[0].start.unix(), e.start.unix());
    }

    #[test]
    fn writes_valid_events() {
        let start = to_unix(days_from_civil(2026, 12, 24), 18 * 3600, &Zone::Utc);
        let ics = write_event(&NewEvent {
            uid: "x@kolibri",
            summary: "Bescherung; mit Komma, und Umlaut ä — ein sehr langer Titel, der umgebrochen werden muss",
            location: "Köln",
            description: "Zeile 1\nZeile 2",
            all_day: false,
            start,
            end: start + 7200,
            now: 0,
            organizer: None,
            attendees: &[],
            sequence: 0,
            method: None,
            cancelled: false,
        });
        assert!(ics.lines().all(|l| l.len() <= 76), "{ics}");
        let cal = parse(&ics);
        let e = &cal.events[0];
        assert_eq!(
            e.summary,
            "Bescherung; mit Komma, und Umlaut ä — ein sehr langer Titel, der umgebrochen werden muss"
        );
        assert_eq!(e.description, "Zeile 1\nZeile 2");
        assert_eq!(cal.instances(0, i64::MAX / 2)[0].end - start, 7200);

        let day = to_unix(days_from_civil(2026, 12, 24), 0, &Zone::Floating);
        let ics = write_event(&NewEvent {
            uid: "y",
            summary: "Heiligabend",
            location: "",
            description: "",
            all_day: true,
            start: day,
            end: day + 86_400,
            now: 0,
            organizer: None,
            attendees: &[],
            sequence: 0,
            method: None,
            cancelled: false,
        });
        assert!(
            ics.contains("DTSTART;VALUE=DATE:20261224")
                && ics.contains("DTEND;VALUE=DATE:20261225"),
            "{ics}"
        );
    }
}
