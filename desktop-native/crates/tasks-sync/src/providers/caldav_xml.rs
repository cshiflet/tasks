//! CalDAV request bodies + response parsers.
//!
//! Keeps the XML in one place so the eventual HTTP client
//! (`providers::caldav`) is a thin wrapper around `reqwest`: build
//! body → PUT/POST → hand the response to the parser here. No I/O
//! in this module; every test is a string fixture.
//!
//! ## Discovery cascade
//!
//! CalDAV service discovery is three round-trips:
//!
//! 1. **PROPFIND /** (Depth 0) with
//!    `<current-user-principal/>` → the server points at the
//!    user's principal URL.
//! 2. **PROPFIND {principal}** (Depth 0) with
//!    `<calendar-home-set/>` → the server points at the
//!    user's calendar collection root.
//! 3. **PROPFIND {home-set}** (Depth 1) with
//!    `<resourcetype/> <displayname/> <calendar-color/> <getctag/>` →
//!    iterate the nested `<response>`s; each child whose
//!    resourcetype contains `<calendar/>` is a list the user
//!    wants surfaced in the sidebar.
//!
//! ## Listing + pulling tasks
//!
//! `calendar-query` REPORT on a calendar URL returns every
//! matching VTODO's href + etag + `calendar-data` (the inlined
//! VCALENDAR). The parser yields `(href, etag, vcalendar)`
//! tuples; the engine feeds each VCALENDAR into
//! [`crate::parse_vcalendar`] and then into
//! [`crate::vtodo_to_remote_task`].

use quick_xml::events::Event;
use quick_xml::reader::Reader;

use crate::provider::SyncError;

/// PROPFIND body: "tell me your current-user-principal". Depth: 0.
pub const PROPFIND_CURRENT_USER_PRINCIPAL: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:">
  <d:prop>
    <d:current-user-principal/>
  </d:prop>
</d:propfind>
"#;

/// PROPFIND body: "tell me where this principal's calendars
/// live." Depth: 0.
pub const PROPFIND_CALENDAR_HOME_SET: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop>
    <c:calendar-home-set/>
  </d:prop>
</d:propfind>
"#;

/// PROPFIND body: "list every child collection + its display
/// metadata." Depth: 1 against the calendar-home-set URL.
pub const PROPFIND_CALENDAR_LIST: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:"
            xmlns:c="urn:ietf:params:xml:ns:caldav"
            xmlns:cs="http://calendarserver.org/ns/"
            xmlns:ical="http://apple.com/ns/ical/">
  <d:prop>
    <d:resourcetype/>
    <d:displayname/>
    <cs:getctag/>
    <ical:calendar-color/>
    <c:supported-calendar-component-set/>
    <d:current-user-privilege-set/>
  </d:prop>
</d:propfind>
"#;

/// REPORT body: `calendar-query` filtered to VTODO components.
/// Depth: 1 against a calendar URL.
pub const REPORT_CALENDAR_QUERY_VTODO: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop>
    <d:getetag/>
    <c:calendar-data/>
  </d:prop>
  <c:filter>
    <c:comp-filter name="VCALENDAR">
      <c:comp-filter name="VTODO"/>
    </c:comp-filter>
  </c:filter>
</c:calendar-query>
"#;

/// Build a `calendar-multiget` REPORT body. Used for a targeted
/// refresh when only specific hrefs are known to have changed
/// (e.g., after a `sync-collection` report surfaces a diff).
pub fn report_multiget(hrefs: &[&str]) -> String {
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
<c:calendar-multiget xmlns:d=\"DAV:\" xmlns:c=\"urn:ietf:params:xml:ns:caldav\">\n\
  <d:prop>\n\
    <d:getetag/>\n\
    <c:calendar-data/>\n\
  </d:prop>\n",
    );
    for h in hrefs {
        s.push_str("  <d:href>");
        s.push_str(&escape_xml_text(h));
        s.push_str("</d:href>\n");
    }
    s.push_str("</c:calendar-multiget>\n");
    s
}

// ---------- response parsers ----------

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PrincipalInfo {
    pub href: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CalendarHomeSet {
    pub href: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CalendarListing {
    pub href: String,
    pub display_name: Option<String>,
    pub ctag: Option<String>,
    pub color: Option<String>,
    pub is_calendar: bool,
    pub read_only: bool,
}

/// `(href, etag, calendar_data)` tuples from a `calendar-query`
/// multistatus response. `calendar_data` holds the raw VCALENDAR
/// text the engine feeds to [`crate::parse_vcalendar`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TaskResource {
    pub href: String,
    pub etag: Option<String>,
    pub calendar_data: Option<String>,
}

/// Walk every `<d:response>` inside a 207 multi-status body and
/// invoke `visit` for each. Each visit sees the response's href +
/// a handful of common property texts keyed by local-name
/// (without namespace prefix). Serves as the shared parsing
/// scaffold for the typed helpers below.
fn walk_multistatus<F: FnMut(&ResponseNode)>(body: &str, mut visit: F) -> Result<(), SyncError> {
    let mut reader = Reader::from_str(body);
    reader.config_mut().trim_text(true);
    let mut current: Option<ResponseNode> = None;
    let mut text_stack: Vec<String> = Vec::new();
    let mut in_href = false;
    let mut in_displayname = false;
    let mut in_getetag = false;
    let mut in_calendar_data = false;
    let mut in_calendar_color = false;
    let mut in_getctag = false;
    let mut in_resourcetype = false;
    let mut in_privilege_set = false;
    let mut seen_write = false;
    let mut seen_read = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = local_name(e.name().as_ref());
                match name.as_str() {
                    "response" => current = Some(ResponseNode::default()),
                    "href" => in_href = true,
                    "displayname" => in_displayname = true,
                    "getetag" => in_getetag = true,
                    "calendar-data" => in_calendar_data = true,
                    "calendar-color" => in_calendar_color = true,
                    "getctag" => in_getctag = true,
                    "resourcetype" => in_resourcetype = true,
                    "current-user-privilege-set" => {
                        in_privilege_set = true;
                        seen_write = false;
                        seen_read = false;
                    }
                    "calendar" if in_resourcetype => {
                        if let Some(node) = current.as_mut() {
                            node.is_calendar = true;
                        }
                    }
                    "write" | "write-content" | "all" if in_privilege_set => {
                        seen_write = true;
                    }
                    "read" if in_privilege_set => {
                        seen_read = true;
                    }
                    _ => {}
                }
                text_stack.push(String::new());
            }
            Ok(Event::Empty(e)) => {
                let name = local_name(e.name().as_ref());
                if name == "calendar" && in_resourcetype {
                    if let Some(node) = current.as_mut() {
                        node.is_calendar = true;
                    }
                }
                if in_privilege_set {
                    if name == "write" || name == "write-content" || name == "all" {
                        seen_write = true;
                    }
                    if name == "read" {
                        seen_read = true;
                    }
                }
            }
            Ok(Event::Text(t)) => {
                if let Some(buf) = text_stack.last_mut() {
                    buf.push_str(&t.unescape().unwrap_or_default());
                }
            }
            Ok(Event::CData(t)) => {
                if let Some(buf) = text_stack.last_mut() {
                    buf.push_str(&String::from_utf8_lossy(&t));
                }
            }
            Ok(Event::End(e)) => {
                let name = local_name(e.name().as_ref());
                let text = text_stack.pop().unwrap_or_default();
                if let Some(node) = current.as_mut() {
                    match name.as_str() {
                        "href" if in_href => {
                            node.href = text.trim().to_string();
                            in_href = false;
                        }
                        "displayname" if in_displayname => {
                            if !text.is_empty() {
                                node.displayname = Some(text);
                            }
                            in_displayname = false;
                        }
                        "getetag" if in_getetag => {
                            node.etag = Some(text.trim().trim_matches('"').to_string());
                            in_getetag = false;
                        }
                        "calendar-data" if in_calendar_data => {
                            node.calendar_data = Some(text);
                            in_calendar_data = false;
                        }
                        "calendar-color" if in_calendar_color => {
                            node.color = Some(text.trim().to_string());
                            in_calendar_color = false;
                        }
                        "getctag" if in_getctag => {
                            node.ctag = Some(text.trim().to_string());
                            in_getctag = false;
                        }
                        "resourcetype" => in_resourcetype = false,
                        "current-user-privilege-set" => {
                            // read-only iff we saw a read privilege but
                            // no write privilege. If we saw neither,
                            // leave the field at its default (false)
                            // rather than guessing.
                            if seen_read && !seen_write {
                                node.read_only = true;
                            }
                            in_privilege_set = false;
                        }
                        "response" => {
                            if let Some(n) = current.take() {
                                visit(&n);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(SyncError::Protocol(format!("XML parse: {e}"))),
            _ => {}
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Default)]
struct ResponseNode {
    href: String,
    displayname: Option<String>,
    etag: Option<String>,
    calendar_data: Option<String>,
    color: Option<String>,
    ctag: Option<String>,
    is_calendar: bool,
    read_only: bool,
}

fn local_name(raw: &[u8]) -> String {
    let s = std::str::from_utf8(raw).unwrap_or("");
    match s.rfind(':') {
        Some(i) => s[i + 1..].to_string(),
        None => s.to_string(),
    }
}

/// XML-escape the five dangerous characters for attribute/text
/// contexts. Good enough for the href + color values we embed —
/// not a general-purpose escaper.
fn escape_xml_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

// ---------- typed helpers ----------

pub fn parse_principal_response(body: &str) -> Result<PrincipalInfo, SyncError> {
    let mut info = PrincipalInfo::default();
    walk_multistatus(body, |node| {
        // PROPFIND on "/" returns the server's response for "/"
        // with `current-user-principal/href` inside. Our walker
        // surfaces the first href it sees inside a `<response>`
        // block, which includes nested `current-user-principal` —
        // take the last non-collection href as the principal.
        if info.href.is_none() {
            info.href = Some(node.href.clone());
        }
    })?;
    Ok(info)
}

pub fn parse_calendar_home_set(body: &str) -> Result<CalendarHomeSet, SyncError> {
    // Same shape as principal parsing — the interesting href is
    // the first (and usually only) one.
    let mut info = CalendarHomeSet::default();
    walk_multistatus(body, |node| {
        if info.href.is_none() {
            info.href = Some(node.href.clone());
        }
    })?;
    Ok(info)
}

pub fn parse_calendar_listing(body: &str) -> Result<Vec<CalendarListing>, SyncError> {
    let mut out = Vec::new();
    walk_multistatus(body, |node| {
        // Skip self (the home-set collection itself) and any
        // child that isn't a calendar.
        if node.is_calendar {
            out.push(CalendarListing {
                href: node.href.clone(),
                display_name: node.displayname.clone(),
                ctag: node.ctag.clone(),
                color: node.color.clone(),
                is_calendar: true,
                read_only: node.read_only,
            });
        }
    })?;
    Ok(out)
}

pub fn parse_task_listing(body: &str) -> Result<Vec<TaskResource>, SyncError> {
    let mut out = Vec::new();
    walk_multistatus(body, |node| {
        if node.calendar_data.is_some() {
            out.push(TaskResource {
                href: node.href.clone(),
                etag: node.etag.clone(),
                calendar_data: node.calendar_data.clone(),
            });
        }
    })?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiget_body_shape() {
        let body = report_multiget(&["/dav/c1/abc.ics", "/dav/c1/def.ics"]);
        assert!(body.contains("<c:calendar-multiget"));
        assert!(body.contains("<d:href>/dav/c1/abc.ics</d:href>"));
        assert!(body.contains("<d:href>/dav/c1/def.ics</d:href>"));
    }

    #[test]
    fn principal_response_extracts_href() {
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/dav/</d:href>
    <d:propstat>
      <d:prop>
        <d:current-user-principal>
          <d:href>/dav/principals/alice/</d:href>
        </d:current-user-principal>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;
        let info = parse_principal_response(xml).unwrap();
        // The walker overwrites `href` each time a `<d:href>`
        // closes inside the current `<d:response>`. That's
        // exactly what we want: the last href seen is the
        // nested `<current-user-principal>/<href>`, which is
        // the principal URL the next PROPFIND targets.
        assert_eq!(info.href.as_deref(), Some("/dav/principals/alice/"));
    }

    #[test]
    fn calendar_listing_parses_multiple_calendars() {
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:"
               xmlns:c="urn:ietf:params:xml:ns:caldav"
               xmlns:cs="http://calendarserver.org/ns/"
               xmlns:ical="http://apple.com/ns/ical/">
  <d:response>
    <d:href>/dav/cals/</d:href>
    <d:propstat>
      <d:prop>
        <d:resourcetype>
          <d:collection/>
        </d:resourcetype>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
  <d:response>
    <d:href>/dav/cals/work/</d:href>
    <d:propstat>
      <d:prop>
        <d:resourcetype>
          <d:collection/>
          <c:calendar/>
        </d:resourcetype>
        <d:displayname>Work</d:displayname>
        <cs:getctag>ctag-abc</cs:getctag>
        <ical:calendar-color>#ff0000</ical:calendar-color>
        <d:current-user-privilege-set>
          <d:privilege><d:read/></d:privilege>
          <d:privilege><d:write/></d:privilege>
        </d:current-user-privilege-set>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
  <d:response>
    <d:href>/dav/cals/readonly/</d:href>
    <d:propstat>
      <d:prop>
        <d:resourcetype>
          <d:collection/>
          <c:calendar/>
        </d:resourcetype>
        <d:displayname>Shared</d:displayname>
        <d:current-user-privilege-set>
          <d:privilege><d:read/></d:privilege>
        </d:current-user-privilege-set>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;
        let cals = parse_calendar_listing(xml).unwrap();
        assert_eq!(cals.len(), 2, "parent collection should be skipped");
        assert_eq!(cals[0].href, "/dav/cals/work/");
        assert_eq!(cals[0].display_name.as_deref(), Some("Work"));
        assert_eq!(cals[0].ctag.as_deref(), Some("ctag-abc"));
        assert_eq!(cals[0].color.as_deref(), Some("#ff0000"));
        assert!(!cals[0].read_only);
        assert_eq!(cals[1].href, "/dav/cals/readonly/");
        assert!(cals[1].read_only);
    }

    #[test]
    fn task_listing_parses_etag_and_calendar_data() {
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:response>
    <d:href>/dav/cals/work/abc.ics</d:href>
    <d:propstat>
      <d:prop>
        <d:getetag>"etag-1"</d:getetag>
        <c:calendar-data>BEGIN:VCALENDAR
BEGIN:VTODO
UID:abc-1
SUMMARY:Buy milk
END:VTODO
END:VCALENDAR</c:calendar-data>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;
        let tasks = parse_task_listing(xml).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].href, "/dav/cals/work/abc.ics");
        assert_eq!(tasks[0].etag.as_deref(), Some("etag-1"));
        let data = tasks[0].calendar_data.as_deref().unwrap();
        assert!(data.contains("UID:abc-1"));
        assert!(data.contains("SUMMARY:Buy milk"));
    }

    #[test]
    fn task_listing_skips_responses_without_calendar_data() {
        // A 404 response inside the multistatus should not
        // surface as a task.
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:response>
    <d:href>/dav/cals/work/missing.ics</d:href>
    <d:status>HTTP/1.1 404 Not Found</d:status>
  </d:response>
</d:multistatus>"#;
        let tasks = parse_task_listing(xml).unwrap();
        assert!(tasks.is_empty());
    }

    #[test]
    fn malformed_xml_surfaces_as_protocol_error() {
        // quick-xml tolerates truncation by EOF-ing cleanly, so
        // force a real parser error with an invalid attribute
        // (unclosed quote).
        let broken = "<d:response foo=\"unterminated";
        let err = parse_calendar_listing(broken).unwrap_err();
        assert!(matches!(err, SyncError::Protocol(_)));
    }

    #[test]
    fn xml_escape_handles_dangerous_chars() {
        assert_eq!(escape_xml_text("a < b & c"), "a &lt; b &amp; c");
        assert_eq!(escape_xml_text("\"quoted\""), "&quot;quoted&quot;");
    }
}
